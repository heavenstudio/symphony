use super::*;
use crate::usage::merged;
use crate::usage::transcript::Ledger;
use serde_json::json;
use std::fs;
use std::path::PathBuf;

/// Claude Opus 5.5's list prices as read 2026-11-16, at both speeds: a fixture of the table's
/// shape, so a test's dollars stay what they are when the embedded table is edited.
const OPUS: &str = r#"
read = "2026-11-16"
page = "https://platform.claude.com/docs/en/about-claude/pricing"

[[price]]
model = "claude-opus-5-5"
speed = "standard"
input = 4
cache_write_5m = 5
cache_write_1h = 8
cache_read = 0.20
output = 20

[[price]]
model = "claude-opus-5-5"
speed = "fast"
input = 8
cache_write_5m = 10
cache_write_1h = 16
cache_read = 0.40
output = 40
"#;

fn opus() -> Prices {
    Prices::parse(OPUS).unwrap()
}

fn tokens(input: u64, five: u64, hour: u64, read: u64, output: u64) -> Tokens {
    Tokens {
        input,
        cache_write_5m: five,
        cache_write_1h: hour,
        cache_read: read,
        output,
        calls: 1,
    }
}

/// A folder of the test's own, removed when it ends.
struct Folder(PathBuf);

impl Folder {
    fn new(name: &str) -> Folder {
        let dir = std::env::temp_dir().join(format!("price-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Folder(dir)
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// An assistant line of message `id` at `speed`, as Claude Code writes one for each content block.
fn assistant(id: &str, speed: &str, t: [u64; 5]) -> String {
    let [input, five, hour, read, output] = t;
    json!({
        "type": "assistant", "sessionId": "s-1", "isSidechain": false,
        "message": {
            "id": id, "model": "claude-opus-5-5", "role": "assistant",
            "content": [{"type": "text", "text": "done"}],
            "usage": {
                "input_tokens": input,
                "cache_creation_input_tokens": five + hour,
                "cache_read_input_tokens": read,
                "output_tokens": output,
                "cache_creation": {"ephemeral_5m_input_tokens": five, "ephemeral_1h_input_tokens": hour},
                "service_tier": "standard", "speed": speed,
            },
        },
    })
    .to_string()
}

#[test]
fn the_embedded_table_reads_and_prices_the_models_agents_run_on() {
    let prices = Prices::parse(TABLE).unwrap();
    assert_eq!(Prices::embedded(), &prices);
    assert!(is_date(&prices.read), "{}", prices.read);
    assert!(prices.page.starts_with("https://"), "{}", prices.page);
    for (model, speed) in [
        ("claude-fable-5-2", "standard"),
        ("claude-fable-5-2", "fast"),
        ("claude-opus-5-5", "standard"),
        ("claude-opus-5-5", "fast"),
        ("claude-haiku-5-5", "standard"),
    ] {
        let price = prices.of(model, speed);
        assert!(
            price.is_some_and(|p| p.input > 0 && p.output > p.input),
            "{model} {speed}"
        );
    }
    // Each price as the page gives it: the cache read is 0.025 of the input on Fable 5.2 and 0.05
    // on Opus 5.5, so no multiplier would give both.
    let fable = prices.of("claude-fable-5-2", "standard").unwrap();
    let opus = prices.of("claude-opus-5-5", "standard").unwrap();
    assert_eq!((fable.input, fable.cache_read), (8_000_000, 200_000));
    assert_eq!((opus.input, opus.cache_read), (4_000_000, 200_000));
}

#[test]
fn a_transcript_of_the_four_kinds_both_lifetimes_and_a_fast_call_costs_its_dollars_to_the_cent() {
    let folder = Folder::new("four-kinds");
    let transcript = folder.0.join("s-1.jsonl");
    let lines = [
        // One message written on two lines, as Claude Code writes a message of two blocks: once.
        assistant("m1", "standard", [1_200, 30_000, 20_000, 500_000, 4_000]),
        assistant("m1", "standard", [1_200, 30_000, 20_000, 500_000, 4_000]),
        assistant("m2", "standard", [300, 0, 0, 600_000, 2_500]),
        assistant("m3", "fast", [100, 10_000, 0, 200_000, 3_000]),
    ];
    fs::write(&transcript, lines.join("\n") + "\n").unwrap();
    let ledger = Ledger::open(&folder.0.join("ledgers"), "k", &transcript).unwrap();
    let spent = merged(ledger.spent().to_vec());
    let prices = opus();
    let mut cost = Cost::default();
    for one in &spent {
        cost.add(&prices.cost(&one.model, &one.speed, &one.tokens));
    }
    // Standard: input 1,500 at $4 = $0.006, 5m writes 30k at $5 = $0.15, 1h writes 20k at $8 =
    // $0.16, reads 1.1M at $0.20 = $0.22, output 6,500 at $20 = $0.13. Fast: input 100 at $8 =
    // $0.0008, 5m writes 10k at $10 = $0.10, reads 200k at $0.40 = $0.08, output 3k at $40 = $0.12.
    assert_eq!(cost.input, 6_800_000_000);
    assert_eq!(cost.cache_write_5m, 250_000_000_000);
    assert_eq!(cost.cache_write_1h, 160_000_000_000);
    assert_eq!(cost.cache_read, 300_000_000_000);
    assert_eq!(cost.output, 250_000_000_000);
    assert_eq!(cost.total(), 966_800_000_000);
    assert_eq!(dollars(cost.total()), "$0.97");
    assert_eq!(cost.said(), "$0.97");
    assert!(cost.unpriced.is_empty());
}

#[test]
fn a_model_the_table_lacks_is_named_and_in_no_dollar_figure() {
    let prices = opus();
    let priced = prices.cost("claude-opus-5-5", "standard", &tokens(0, 0, 0, 0, 1_000));
    let mut cost = priced.clone();
    cost.add(&prices.cost(
        "gpt-6.1-sol",
        "standard",
        &tokens(1_000, 0, 0, 200_000, 500),
    ));
    // Opus 5.5 by its exact id: another of its family, or a speed the table lacks, is unpriced.
    cost.add(&prices.cost("claude-opus-5-6", "standard", &tokens(10, 0, 0, 0, 0)));
    cost.add(&prices.cost("claude-opus-5-5", "priority", &tokens(0, 0, 0, 0, 5)));
    assert_eq!(cost.total(), priced.total());
    assert_eq!(cost.said(), "$0.02 + unpriced");
    assert_eq!(
        cost.unpriced_said(" · "),
        "claude-opus-5-5 priority 5 tokens · claude-opus-5-6 10 tokens · gpt-6.1-sol 201k tokens"
    );
    let none = prices.cost("gpt-6.1-sol", "standard", &tokens(1, 0, 0, 0, 0));
    assert_eq!((none.total(), none.said()), (0, "unpriced".to_string()));
    // No tokens of an unknown model is nothing to name.
    assert_eq!(
        prices.cost("gpt-6.1-sol", "standard", &Tokens::default()),
        Cost::default()
    );
}

#[test]
fn unpriced_tokens_of_one_model_and_speed_add_up() {
    let prices = opus();
    let mut cost = prices.cost("gpt-x", "standard", &tokens(1, 2, 3, 4, 5));
    cost.add(&prices.cost("gpt-x", "standard", &tokens(10, 20, 30, 40, 50)));
    assert_eq!(cost.unpriced.len(), 1);
    let both = &cost.unpriced[&("gpt-x".to_string(), "standard".to_string())];
    assert_eq!((both.total(), both.calls), (165, 2));
}

#[test]
fn dollars_round_to_the_cent_half_a_cent_up() {
    let cent = PICO_PER_CENT;
    for (pico, said) in [
        (0, "$0.00"),
        (cent / 2 - 1, "$0.00"),
        (cent / 2, "$0.01"),
        (cent, "$0.01"),
        (7 * 100 * cent + 27 * cent + cent / 3, "$7.27"),
        (123_450 * cent, "$1234.50"),
        (99 * cent + cent / 2, "$1.00"),
    ] {
        assert_eq!(dollars(pico), said, "{pico}");
    }
}

#[test]
fn the_json_gives_dollars_of_each_kind_the_unpriced_and_the_day_the_prices_were_read() {
    let prices = opus();
    let mut cost = prices.cost(
        "claude-opus-5-5",
        "standard",
        &tokens(250_000, 100_000, 0, 1_000_000, 50_000),
    );
    cost.add(&prices.cost("gpt-x", "fast", &tokens(7, 0, 0, 0, 0)));
    let json = crate::pyjson::dumps_line(&cost.json(&prices.read));
    assert_eq!(
        json,
        "{\"dollars\": 2.7, \"input\": 1.0, \"cache_write_5m\": 0.5, \"cache_write_1h\": 0.0, \
         \"cache_read\": 0.2, \"output\": 1.0, \"unpriced\": [{\"model\": \"gpt-x\", \"speed\": \
         \"fast\", \"tokens\": {\"input\": 7, \"cache_write_5m\": 0, \"cache_write_1h\": 0, \
         \"cache_read\": 0, \"output\": 0, \"calls\": 1}}], \"prices\": \"2026-11-16\"}"
    );
}

#[test]
fn a_table_written_wrong_is_refused_naming_what_is_wrong() {
    let row = |body: &str| {
        format!("read = \"2026-11-16\"\npage = \"p\"\n[[price]]\nmodel = \"m\"\n{body}\n")
    };
    let all = "input = 1\ncache_write_5m = 1\ncache_write_1h = 1\ncache_read = 1\noutput = 1";
    assert!(Prices::parse(&row(all)).is_ok());
    // A row with no speed is at standard speed.
    assert!(
        Prices::parse(&row(all))
            .unwrap()
            .of("m", "standard")
            .is_some()
    );
    for (text, says) in [
        (row(&format!("{all}\nbatch = 1")), "unknown field"),
        (row("input = 1"), "missing field"),
        (
            row(&all.replace("input = 1", "input = -1")),
            "input for m at standard speed is -1",
        ),
        (
            row(&all.replace("output = 1", "output = nan")),
            "output for m at standard speed",
        ),
        (
            row(&all.replace("output = 1", "output = 2e6")),
            "not a price from 0 to",
        ),
        (
            format!("{}[[price]]\nmodel = \"m\"\n{all}\n", row(all)),
            "prices m at standard speed twice",
        ),
        (row(&format!("speed = \"\"\n{all}")), "no model or no speed"),
        (row(all).replace("2026-11-16", "16/11/2026"), "is no date"),
        (
            "read = \"2026-11-16\"\n".to_string(),
            "missing field `page`",
        ),
    ] {
        let error = Prices::parse(&text).unwrap_err();
        assert!(error.contains(says), "{says:?} not in {error:?}");
    }
}

#[test]
fn a_key_of_the_wrong_type_or_place_is_refused_naming_it_and_its_row() {
    let all = "input = 1\ncache_write_5m = 1\ncache_write_1h = 1\ncache_read = 1\noutput = 1";
    let rows = |rows: &str| format!("read = \"2026-11-16\"\npage = \"p\"\n{rows}");
    let row = |body: &str| rows(&format!("[[price]]\nmodel = \"m\"\n{body}\n"));
    let second = |body: &str| format!("{}[[price]]\nmodel = \"n\"\n{body}\n", row(all));
    for (text, says) in [
        (
            row(&all.replace("input = 1", "input = \"1\"")),
            "invalid type: string for `input` in [[price]] 1, expected a number",
        ),
        (
            row(&format!("{all}\nspeed = true")),
            "invalid type: boolean for `speed` in [[price]] 1, expected a string",
        ),
        (
            rows(&format!("[[price]]\nmodel = 5\n{all}\n")),
            "invalid type: integer for `model` in [[price]] 1, expected a string",
        ),
        (
            second(&format!("{all}\nbatch = 1")),
            "unknown field `batch` in [[price]] 2, expected one of `model`, `speed`, `input`, \
             `cache_write_5m`, `cache_write_1h`, `cache_read`, `output`",
        ),
        (
            second("input = 1"),
            "missing field `cache_write_5m` in [[price]] 2",
        ),
        (
            rows(&format!("[[price]]\n{all}\n")),
            "missing field `model` in [[price]] 1",
        ),
        ("read = \"2026-11-16\n".to_string(), "TOML parse error"),
        (
            format!("color = 1\n{}", rows("")),
            "unknown field `color`, expected one of `read`, `page`, `price`",
        ),
        ("page = \"p\"\n".to_string(), "missing field `read`"),
        (
            rows("").replace("\"2026-11-16\"", "2026-11-16"),
            "invalid type: datetime for `read`, expected a string",
        ),
        (
            rows("price = 5\n"),
            "invalid type: integer for `price`, expected an array of tables",
        ),
        (
            rows("price = [1]\n"),
            "invalid type: integer in [[price]] 1, expected a table",
        ),
    ] {
        let error = Prices::parse(&text).unwrap_err();
        assert!(
            error.starts_with("the price table does not read: ") && error.contains(says),
            "{says:?} not in {error:?}"
        );
    }
    // The rows written inline are the same rows, and a table with none prices nothing.
    let inline = rows(&format!(
        "price = [{{ model = \"m\", {} }}]\n",
        all.replace('\n', ", ")
    ));
    assert_eq!(Prices::parse(&inline), Prices::parse(&row(all)));
    assert_eq!(Prices::parse(&rows("")).unwrap().of("m", "standard"), None);
}

#[test]
fn a_price_counts_to_the_millionth_of_a_dollar() {
    let text = "read = \"2026-11-16\"\npage = \"p\"\n[[price]]\nmodel = \"m\"\ninput = 0.125\n\
                cache_write_5m = 0.000001\ncache_write_1h = 0\ncache_read = 0.01\noutput = 0.5";
    let price = Prices::parse(text).unwrap().of("m", "standard").unwrap();
    assert_eq!(
        price,
        Price {
            input: 125_000,
            cache_write_5m: 1,
            cache_write_1h: 0,
            cache_read: 10_000,
            output: 500_000,
        }
    );
}
