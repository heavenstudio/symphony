//! Reproducible, explicit scratch-board measurement. Reads the live board over RPC without opening
//! or upgrading its schema, saves the snapshot, and writes only the named non-live database.
use serde_json::json;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;
use tasks::embedding::{Bge, Embedder, MODEL_ID, fuse};
use tasks::sys::Spawning as _;
use tasks::{DATABASE, cli, rows::Dump, rpc};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let database = args
        .windows(2)
        .find(|a| a[0] == "--database")
        .map(|a| a[1].as_str())
        .ok_or("--database NAME is required")?;
    if database == DATABASE || !rpc::plain(database) {
        return Err("benchmark requires an explicit non-live database".into());
    }
    let queries_only = args.iter().any(|a| a == "--queries-only");
    if queries_only && !args.iter().any(|a| a == "--resume") {
        return Err(
            "--queries-only requires --resume and an already embedded scratch board".into(),
        );
    }
    let out = PathBuf::from(
        args.windows(2)
            .find(|a| a[0] == "--out")
            .map(|a| a[1].as_str())
            .ok_or("--out DIR is required")?,
    );
    fs::create_dir_all(&out)?;
    let env = cli::Env::of_process();
    let live = cli::board_client(&env)?;
    let mut scratch = env.clone();
    scratch
        .vars
        .insert("TASKS_DATABASE".into(), database.into());
    scratch.vars.insert("TASKS_SWEEP".into(), "off".into());
    let store = cli::connect(&scratch)?;
    if args.iter().any(|a| a == "--maintenance-cost") {
        if !queries_only {
            return Err("--maintenance-cost requires --resume --queries-only".into());
        }
        maintenance_cost(&store, &out, &scratch)?;
        return Ok(());
    }
    if !args.iter().any(|a| a == "--resume") {
        let mut rows = live.query("BEGIN TRANSACTION; RETURN {tasks: (SELECT * OMIT id, version FROM task ORDER BY sid), task_links: (SELECT * OMIT id FROM link ORDER BY slug, kind, target), sessions: (SELECT * OMIT id FROM session ORDER BY handle), roles: (SELECT * OMIT id FROM role ORDER BY slug, handle, role), messages: (SELECT * OMIT id FROM message ORDER BY sid), deliveries: (SELECT * OMIT id FROM delivery ORDER BY message, handle), barred_keys: (SELECT * OMIT id FROM barred_key ORDER BY key), task_sessions: (SELECT * OMIT id FROM task_session ORDER BY slug, handle)}; COMMIT TRANSACTION;", json!({})).map_err(|e| e.to_string())?;
        let dump: Dump = serde_json::from_value(rows.remove(1))?;
        fs::write(out.join("board.json"), serde_json::to_vec(&dump)?)?;
        store.copy(&dump)?;
        println!(
            "copied {} tasks, {} messages into TASKS_DATABASE={database}",
            dump.tasks.len(),
            dump.messages.len()
        );
    }
    let mut report = json!({"database":database,"model":MODEL_ID,"runtime":"tract-onnx 0.22.5","load_before": load()});
    let start = Instant::now();
    let mut model = Bge::load(&tasks::git::common_dir(&env.cwd).map_err(|e| e.to_string())?)?;
    report["load_model_s"] = json!(start.elapsed().as_secs_f64());
    println!("model loaded in {} s", report["load_model_s"]);
    let start = Instant::now();
    let v = model.query("verify slow")?;
    println!(
        "query vector {} dimensions, norm {}",
        v.len(),
        v.iter().map(|v| v * v).sum::<f32>().sqrt()
    );
    report["first_query_s"] = json!(start.elapsed().as_secs_f64());
    let mut times = Vec::new();
    for _ in 0..5 {
        let start = Instant::now();
        model.query("review app cpu")?;
        times.push(start.elapsed().as_secs_f64());
    }
    report["warm_query_s"] = json!(times);
    fs::write(out.join("report.json"), serde_json::to_vec_pretty(&report)?)?;
    if args.iter().any(|a| a == "--model-only") {
        return Ok(());
    }
    let tasks = store.tasks(None)?;
    let mut progress = Instant::now();
    let whole = Instant::now();
    for (name, slugs) in [
        (
            "one_task",
            Some(vec!["tools/verify-wall-budget".to_string()]),
        ),
        (
            "batch_32",
            Some(
                tasks
                    .iter()
                    .filter(|t| t.slug != "tools/verify-wall-budget")
                    .take(32)
                    .map(|t| t.slug.clone())
                    .collect::<Vec<_>>(),
            ),
        ),
        ("whole_board_remainder", None),
    ] {
        if queries_only {
            break;
        }
        let start = Instant::now();
        let stats =
            store.catch_up_embeddings(&mut model, slugs.as_deref(), 8, &mut |done, total| {
                if progress.elapsed().as_secs() >= 10 || done == total {
                    println!("{name}: {done}/{total} chunks");
                    progress = Instant::now();
                }
            })?;
        report[name] =
            json!({"elapsed_s":start.elapsed().as_secs_f64(),"stats":stats,"load_after":load()});
        println!("{name}: {}", report[name]);
        fs::write(out.join("report.json"), serde_json::to_vec_pretty(&report)?)?;
    }
    if !queries_only {
        report["whole_first_catch_up_s"] = json!(whole.elapsed().as_secs_f64());
    }
    let by_slug: BTreeMap<_, _> = tasks.into_iter().map(|t| (t.slug.clone(), t)).collect();
    let mut queries = Vec::new();
    for query in [
        "test",
        "verify slow",
        "nightly",
        "hull art",
        "python",
        "deck",
        "golden",
        "flaky",
        "review app cpu",
        "torpedo",
    ] {
        let keyword = store.search(query)?;
        let start = Instant::now();
        let vector = model.query(query)?;
        let query_s = start.elapsed().as_secs_f64();
        let start = Instant::now();
        let (ann, profile) = store.vector_search_profiled(&vector, MODEL_ID, None, 100, false)?;
        let search_s = start.elapsed().as_secs_f64();
        let exact = store.vector_search(&vector, MODEL_ID, None, 100, true)?;
        let hybrid = fuse(&keyword, &ann, query);
        let rows = |hits: &[tasks::SearchHit]| {
            hits.iter().take(5).map(|h|json!({"slug":h.slug,"score":h.score,"title":by_slug.get(&h.slug).map(|t|&t.title)})).collect::<Vec<_>>()
        };
        let ann_rows: Vec<_> = ann
            .iter()
            .map(|h| tasks::SearchHit {
                slug: h.slug.clone(),
                score: h.score,
            })
            .collect();
        let recall = exact
            .iter()
            .take(5)
            .filter(|h| ann.iter().take(5).any(|a| a.slug == h.slug))
            .count() as f64
            / 5.0;
        queries.push(json!({"query":query,"query_s":query_s,"vector_search_s":search_s,"profile":profile,"keyword":rows(&keyword),"vector":rows(&ann_rows),"hybrid":rows(&hybrid),"exact_top5":exact.iter().take(5).collect::<Vec<_>>(),"recall_at5":recall}));
        report["queries"] = json!(queries);
        fs::write(out.join("report.json"), serde_json::to_vec_pretty(&report)?)?;
        println!("query {query}: {}", queries.last().unwrap());
    }
    let vector = model.query("review app cpu")?;
    report["filtered_ready"] = json!({"ann":store.vector_search(&vector,MODEL_ID,Some("ready"),5,false)?,"exact":store.vector_search(&vector,MODEL_ID,Some("ready"),5,true)?});
    report["load_after"] = json!(load());
    fs::write(out.join("report.json"), serde_json::to_vec_pretty(&report)?)?;
    Ok(())
}

fn load() -> String {
    std::process::Command::new("uptime")
        .output_locked()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|e| e.to_string())
}

fn cpu() -> std::io::Result<f64> {
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: the clock writes one timespec for this process.
    if unsafe { libc::clock_gettime(libc::CLOCK_PROCESS_CPUTIME_ID, &mut time) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(time.tv_sec as f64 + time.tv_nsec as f64 / 1e9)
}

fn maintenance_cost(
    store: &tasks::Store,
    out: &std::path::Path,
    env: &cli::Env,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut env = env.clone();
    env.vars.remove("TASKS_SESSION");
    let pass = || -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        let args = ["embed", "--maintenance", "--threads", "1"].map(String::from);
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let start = Instant::now();
        let before = cpu()?;
        let code = cli::main(&args, &env, &mut &b""[..], &mut out, &mut err);
        let cpu_s = cpu()? - before;
        let wall_s = start.elapsed().as_secs_f64();
        if code != 0 {
            return Err(
                format!("maintenance exit {code}: {}", String::from_utf8_lossy(&err)).into(),
            );
        }
        Ok(json!({"cpu_s":cpu_s,"wall_s":wall_s,"output":String::from_utf8_lossy(&out)}))
    };
    let mut report = json!({"load_before":load(),"cpu_scope":"CLI command body including connection, cache lock and model load when needed; OS process startup and database CPU excluded"});
    let mut idle = Vec::new();
    for _ in 0..5 {
        let result = pass()?;
        if !result["output"]
            .as_str()
            .unwrap()
            .contains("embedded 0 tasks, 0 changed sources, 0 chunks")
        {
            return Err("idle pass changed a source".into());
        }
        idle.push(result);
    }
    report["idle"] = json!(idle);
    // A task with just a new title/brief has no thread backlog to infer. Reuse that frozen brief
    // rather than inventing a synthetic text, leaving every source record unchanged.
    store.client().query("BEGIN TRANSACTION; DELETE task_chunk WHERE slug = $slug AND source = 'brief'; DELETE task_embedding_source WHERE slug = $slug AND source = 'brief'; COMMIT TRANSACTION;",json!({"slug":"tools/verify-wall-budget"})).map_err(|e| e.to_string())?;
    report["new_task_brief"] = pass()?;
    // Only the explicit scratch cache changes: rebuild exactly one previously measured task.
    store.client().query("BEGIN TRANSACTION; DELETE task_chunk WHERE slug = $slug; DELETE task_embedding_source WHERE slug = $slug; COMMIT TRANSACTION;",json!({"slug":"tools/verify-wall-budget"})).map_err(|e| e.to_string())?;
    report["one_task"] = pass()?;
    report["load_after"] = json!(load());
    fs::write(
        out.join("maintenance-cost.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
