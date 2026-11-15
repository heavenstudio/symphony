//! One read of the board's shape (its version and counts, and a column of 30 cards) served three
//! ways on axum 0.8, one per build: a plain JSON read (`rest`), async-graphql or Juniper.
//!
//! - `bench ROUNDS`: in this process, a batch of 1,000 plain reads (the data serialized, as the
//!   server's reads do) and a batch of 1,000 GraphQL queries (parsed, validated, executed and
//!   serialized), taken in turn ROUNDS times; prints each kind's median batch in µs a read.
//! - `serve-once`: listens on a private loopback port, prints its address, answers one request
//!   and exits.

use std::sync::OnceLock;
use std::time::Instant;

use serde::Serialize;

/// The query a GraphQL page would send for what the REST reads `/api/board` and one column give.
#[cfg_attr(feature = "rest", allow(dead_code))]
const QUERY: &str = "query Column($status: String!) {
  board { version counts { ready inProgress review } }
  column(status: $status) { slug title priority }
}";

#[derive(Clone, Serialize)]
#[cfg_attr(feature = "async-graphql", derive(async_graphql::SimpleObject))]
#[cfg_attr(feature = "juniper", derive(juniper::GraphQLObject))]
struct Counts {
    ready: i32,
    in_progress: i32,
    review: i32,
}

#[derive(Clone, Serialize)]
#[cfg_attr(feature = "async-graphql", derive(async_graphql::SimpleObject))]
#[cfg_attr(feature = "juniper", derive(juniper::GraphQLObject))]
struct Board {
    version: i32,
    counts: Counts,
}

#[derive(Clone, Serialize)]
#[cfg_attr(feature = "async-graphql", derive(async_graphql::SimpleObject))]
#[cfg_attr(feature = "juniper", derive(juniper::GraphQLObject))]
struct Task {
    slug: String,
    title: String,
    priority: String,
    status: String,
    created: String,
    updated: String,
}

/// What a plain read sends: the board and the column, every field of each card.
#[derive(Serialize)]
struct Page<'a> {
    board: &'a Board,
    column: &'a [Task],
}

struct Data {
    board: Board,
    tasks: Vec<Task>,
}

fn data() -> &'static Data {
    static DATA: OnceLock<Data> = OnceLock::new();
    DATA.get_or_init(|| Data {
        board: Board {
            version: 4182,
            counts: Counts {
                ready: 30,
                in_progress: 6,
                review: 3,
            },
        },
        tasks: (0..30)
            .map(|n| Task {
                slug: format!("app/task-number-{n}"),
                title: format!("A task of the board, its title about this long, number {n}"),
                priority: format!("p{}", n % 3),
                status: "ready".into(),
                created: "2026-11-14T10:31:00Z".into(),
                updated: "2026-11-14T10:31:00Z".into(),
            })
            .collect(),
    })
}

#[cfg_attr(feature = "rest", allow(dead_code))]
fn column(status: &str) -> Vec<Task> {
    data()
        .tasks
        .iter()
        .filter(|task| task.status == status)
        .cloned()
        .collect()
}

fn plain_read() -> Vec<u8> {
    let data = data();
    serde_json::to_vec(&Page {
        board: &data.board,
        column: &data.tasks,
    })
    .expect("serializes")
}

#[cfg(feature = "async-graphql")]
mod graphql {
    use async_graphql::{EmptyMutation, EmptySubscription, Object, Request, Schema, Variables};

    pub struct Query;

    #[Object]
    impl Query {
        async fn board(&self) -> super::Board {
            super::data().board.clone()
        }
        async fn column(&self, status: String) -> Vec<super::Task> {
            super::column(&status)
        }
    }

    pub type Built = Schema<Query, EmptyMutation, EmptySubscription>;

    pub fn schema() -> Built {
        Schema::new(Query, EmptyMutation, EmptySubscription)
    }

    pub async fn query(schema: &Built) -> Vec<u8> {
        let variables = Variables::from_json(serde_json::json!({ "status": "ready" }));
        let response = schema
            .execute(Request::new(super::QUERY).variables(variables))
            .await;
        assert!(response.errors.is_empty(), "{:?}", response.errors);
        serde_json::to_vec(&response).expect("serializes")
    }

    pub fn router() -> axum::Router {
        axum::Router::new().route_service("/graphql", async_graphql_axum::GraphQL::new(schema()))
    }
}

#[cfg(feature = "juniper")]
mod graphql {
    use juniper::{EmptyMutation, EmptySubscription, InputValue, RootNode, Variables, graphql_object};
    use std::sync::Arc;

    #[derive(Clone, Default)]
    pub struct Context;
    impl juniper::Context for Context {}

    pub struct Query;

    #[graphql_object]
    #[graphql(context = Context)]
    impl Query {
        fn board() -> super::Board {
            super::data().board.clone()
        }
        fn column(status: String) -> Vec<super::Task> {
            super::column(&status)
        }
    }

    pub type Built = RootNode<Query, EmptyMutation<Context>, EmptySubscription<Context>>;

    pub fn schema() -> Built {
        RootNode::new(Query, EmptyMutation::new(), EmptySubscription::new())
    }

    pub async fn query(schema: &Built) -> Vec<u8> {
        let mut variables = Variables::new();
        variables.insert("status".into(), InputValue::scalar("ready"));
        let (value, errors) = juniper::execute(super::QUERY, None, schema, &variables, &Context)
            .await
            .expect("a valid query");
        assert!(errors.is_empty(), "{errors:?}");
        serde_json::to_vec(&value).expect("serializes")
    }

    pub fn router() -> axum::Router {
        axum::Router::new()
            .route(
                "/graphql",
                axum::routing::post(juniper_axum::graphql::<Arc<Built>>),
            )
            .layer(axum::Extension(Arc::new(schema())))
    }
}

#[cfg(feature = "rest")]
mod graphql {
    pub struct Built;

    pub fn schema() -> Built {
        Built
    }

    /// No GraphQL in this build: the plain read again, so the bench prints both alike.
    pub async fn query(_: &Built) -> Vec<u8> {
        super::plain_read()
    }

    pub fn router() -> axum::Router {
        axum::Router::new().route(
            "/api/column",
            axum::routing::get(|| async { super::plain_read() }),
        )
    }
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

async fn bench(rounds: usize) {
    const BATCH: u32 = 1_000;
    let schema = graphql::schema();
    let (plain_bytes, graphql_bytes) = (plain_read().len(), graphql::query(&schema).await.len());
    let (mut plain, mut queried) = (Vec::new(), Vec::new());
    for _ in 0..rounds {
        let start = Instant::now();
        for _ in 0..BATCH {
            std::hint::black_box(plain_read());
        }
        plain.push(start.elapsed().as_secs_f64() * 1e6 / f64::from(BATCH));
        let start = Instant::now();
        for _ in 0..BATCH {
            std::hint::black_box(graphql::query(&schema).await);
        }
        queried.push(start.elapsed().as_secs_f64() * 1e6 / f64::from(BATCH));
    }
    println!(
        "plain read: {:.1} µs ({plain_bytes} bytes); query: {:.1} µs ({graphql_bytes} bytes); \
         medians of {rounds} batches of {BATCH}",
        median(plain),
        median(queried)
    );
}

async fn serve_once() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("binds");
    println!("http://{}", listener.local_addr().expect("has an address"));
    let (done, ended) = tokio::sync::oneshot::channel::<()>();
    let done = std::sync::Arc::new(std::sync::Mutex::new(Some(done)));
    let app = graphql::router().layer(axum::middleware::from_fn(
        move |request: axum::extract::Request, next: axum::middleware::Next| {
            let sent = done.lock().expect("not poisoned").take();
            async move {
                let response = next.run(request).await;
                if let Some(sent) = sent {
                    let _ = sent.send(());
                }
                response
            }
        },
    ));
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = ended.await;
        })
        .await
        .expect("serves");
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("bench") => bench(args.get(1).and_then(|n| n.parse().ok()).unwrap_or(15)).await,
        Some("serve-once") => serve_once().await,
        _ => eprintln!("usage: graphql-stack-server bench [ROUNDS] | serve-once"),
    }
}
