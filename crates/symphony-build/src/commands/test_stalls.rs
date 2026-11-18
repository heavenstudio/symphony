//! A streaming annotation of the pinned runner's timeout events, never another test runner.
//! Every other byte passes unchanged. The configured deadlines are read from the release
//! profile; a timeout says it was taken as a stall, which deadline it reached and the load then.

use std::io::{BufRead, Write};

const STANDALONE: &str = "package(=symphony-server) & binary(=standalone)";
const USAGE: &str = "usage: symphony-build test-stalls CONFIG\n";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Limits {
    ordinary: u64,
    standalone: u64,
}

fn deadline(value: &toml::Value) -> Result<u64, String> {
    let seconds = value
        .get("period")
        .and_then(toml::Value::as_str)
        .and_then(|period| period.strip_suffix('s'))
        .and_then(|seconds| seconds.parse::<u64>().ok())
        .filter(|seconds| *seconds > 0)
        .ok_or("a backstop period must be positive whole seconds")?;
    let periods = value
        .get("terminate-after")
        .and_then(toml::Value::as_integer)
        .and_then(|periods| u64::try_from(periods).ok())
        .filter(|periods| *periods > 0)
        .ok_or("a backstop needs a positive terminate-after")?;
    seconds
        .checked_mul(periods)
        .ok_or("backstop overflow".into())
}

fn limits(text: &str) -> Result<Limits, String> {
    let config: toml::Value = toml::from_str(text).map_err(|why| why.to_string())?;
    let profile = config
        .get("profile")
        .and_then(|profiles| profiles.get("release"))
        .ok_or("missing release profile")?;
    let ordinary = deadline(
        profile
            .get("slow-timeout")
            .ok_or("missing default backstop")?,
    )?;
    let mut standalone = None;
    if let Some(overrides) = profile.get("overrides") {
        for rule in overrides.as_array().ok_or("overrides must be an array")? {
            let Some(timeout) = rule.get("slow-timeout") else {
                continue;
            };
            if rule.get("filter").and_then(toml::Value::as_str) != Some(STANDALONE) {
                return Err("an unknown timeout override cannot be attributed".into());
            }
            if standalone.replace(deadline(timeout)?).is_some() {
                return Err("ambiguous standalone timeout overrides".into());
            }
        }
    }
    Ok(Limits {
        ordinary,
        standalone: standalone.unwrap_or(ordinary),
    })
}

fn relay(
    input: &mut dyn BufRead,
    out: &mut dyn Write,
    limits: Limits,
    mut load: impl FnMut() -> String,
) -> std::io::Result<()> {
    let mut line = Vec::new();
    loop {
        line.clear();
        if input.read_until(b'\n', &mut line)? == 0 {
            return Ok(());
        }
        let event = if line.starts_with(b"{") {
            serde_json::from_slice::<serde_json::Value>(&line).ok()
        } else {
            None
        };
        if let Some(mut event) = event
            && event.get("type").and_then(|v| v.as_str()) == Some("test")
            && event.get("event").and_then(|v| v.as_str()) == Some("failed")
            && event.get("reason").and_then(|v| v.as_str()) == Some("time limit exceeded")
            && let Some(name) = event.get("name").and_then(|v| v.as_str())
            && name
                .split_once('$')
                .is_some_and(|(suite, case)| !suite.is_empty() && !case.is_empty())
        {
            let seconds = if name.starts_with("symphony-server::standalone$") {
                limits.standalone
            } else {
                limits.ordinary
            };
            event["symphony-stall"] = serde_json::json!({
                "limit_seconds": seconds,
                "load": load(),
            });
            serde_json::to_writer(&mut *out, &event)?;
            if line.ends_with(b"\n") {
                out.write_all(b"\n")?;
            }
        } else {
            out.write_all(&line)?;
        }
        out.flush()?;
    }
}

pub(crate) fn main(
    args: &[String],
    input: &mut dyn BufRead,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    if args.len() == 1 && matches!(args[0].as_str(), "--help" | "-h") {
        let _ = write!(out, "{USAGE}");
        return 0;
    }
    let [config] = args else {
        let _ = write!(err, "{USAGE}");
        return 2;
    };
    let result = std::fs::read_to_string(config)
        .map_err(|why| why.to_string())
        .and_then(|text| limits(&text))
        .and_then(|limits| {
            relay(input, out, limits, crate::release::test_load).map_err(|why| why.to_string())
        });
    match result {
        Ok(()) => 0,
        Err(why) => {
            let _ = writeln!(err, "test-stalls: error: {why}");
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_tree::Tree;

    const CONFIG: &str = "[profile.release]\nslow-timeout={period='132s',terminate-after=3}\n\
        [[profile.release.overrides]]\nfilter='package(=symphony-server) & binary(=standalone)'\n\
        slow-timeout={period='870s',terminate-after=3}\n";

    #[test]
    fn production_backstops_cover_three_times_the_recorded_loaded_upper_bounds() {
        let got = limits(include_str!("../../../../.config/nextest.toml")).unwrap();
        assert!(got.ordinary >= 3 * 132); // ceil(131.95s), loaded ordinary-binary upper bound.
        assert!(got.standalone >= 3 * 870); // ceil(869.16s), loaded standalone binary.
    }

    #[test]
    fn deadlines_come_from_the_actual_profile_and_unknown_or_ambiguous_rules_refuse() {
        assert_eq!(
            limits(CONFIG).unwrap(),
            Limits {
                ordinary: 396,
                standalone: 2610
            }
        );
        let plain = "[profile.release]\nslow-timeout={period='1s',terminate-after=3}";
        assert_eq!(
            limits(plain).unwrap(),
            Limits {
                ordinary: 3,
                standalone: 3
            }
        );
        for bad in [
            "{".into(),
            "[profile.other]".into(),
            "[profile.release]".into(),
            "[profile.release]\nslow-timeout={period='1s'}".into(),
            "[profile.release]\nslow-timeout={terminate-after=3}".into(),
            "[profile.release]\nslow-timeout={period='1s',terminate-after=3}\noverrides='invalid'"
                .into(),
            CONFIG.replace("132s", "1ms"),
            CONFIG.replace("132s", "0s"),
            CONFIG.replace("terminate-after=3", "terminate-after=0"),
            CONFIG.replace("terminate-after=3", "terminate-after=-1"),
            CONFIG.replace("132s", "18446744073709551615s"),
            CONFIG.replace(STANDALONE, "test(other)"),
            format!(
                "{CONFIG}[[profile.release.overrides]]\nfilter='{STANDALONE}'\nslow-timeout={{period='1s',terminate-after=3}}"
            ),
        ] {
            assert!(limits(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn only_timeout_events_gain_their_actual_deadline_and_observed_load() {
        for (name, seconds) in [
            ("p::case$stalled", 396),
            ("symphony-server::standalone$compile", 2610),
        ] {
            let event = serde_json::json!({"type":"test","event":"failed","reason":"time limit exceeded","name":name,"exec_time":1.5});
            let input = format!("{event}\n");
            let mut out = Vec::new();
            relay(
                &mut input.as_bytes(),
                &mut out,
                limits(CONFIG).unwrap(),
                || "123.4, 111.2, 100.0".into(),
            )
            .unwrap();
            let got: serde_json::Value = serde_json::from_slice(&out).unwrap();
            assert_eq!(got["symphony-stall"]["limit_seconds"], seconds);
            assert_eq!(got["symphony-stall"]["load"], "123.4, 111.2, 100.0");
            assert_eq!(got["name"], event["name"]);
            assert_eq!(got["exec_time"], event["exec_time"]);
        }
        let input=b"plain\n\xff\n{bad}\n {\"type\":\"test\"}\n{\"type\":\"test\",\"event\":\"failed\",\"stdout\":\"time limit exceeded\"}\n";
        let mut out = Vec::new();
        relay(&mut &input[..], &mut out, limits(CONFIG).unwrap(), || {
            panic!("no timeout")
        })
        .unwrap();
        assert_eq!(out, input);
        let input=b"{\"type\":\"test\",\"event\":\"failed\",\"reason\":\"time limit exceeded\",\"name\":\"p::case$stalled\"}";
        let mut out = Vec::new();
        relay(&mut &input[..], &mut out, limits(CONFIG).unwrap(), || {
            "unavailable".into()
        })
        .unwrap();
        assert!(!out.ends_with(b"\n"));
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&out).unwrap()["symphony-stall"]["load"],
            "unavailable"
        );
    }

    #[test]
    fn help_bad_arguments_unreadable_config_and_io_errors_keep_their_status() {
        let tree = Tree::new();
        let config = tree.write("config.toml", CONFIG);
        for (args, code) in [
            (vec!["--help".into()], 0),
            (vec!["-h".into()], 0),
            (vec![], 2),
            (vec!["a".into(), "b".into()], 2),
            (vec![tree.path("missing").display().to_string()], 2),
            (vec![config.display().to_string()], 0),
        ] {
            let (mut out, mut err) = (Vec::new(), Vec::new());
            assert_eq!(
                main(&args, &mut &b"ordinary\n"[..], &mut out, &mut err),
                code
            );
            if code == 0 {
                assert!(err.is_empty());
            } else {
                assert!(!err.is_empty());
            }
        }
        struct Broken;
        impl Write for Broken {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("broken output"))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut err = Vec::new();
        assert_eq!(
            main(
                &[config.display().to_string()],
                &mut &b"line\n"[..],
                &mut Broken,
                &mut err
            ),
            2
        );
        assert!(String::from_utf8(err).unwrap().contains("broken output"));
        let input=b"{\"type\":\"test\",\"event\":\"failed\",\"reason\":\"time limit exceeded\",\"name\":\"p::case$stalled\"}\n";
        let mut out = Vec::new();
        let mut err = Vec::new();
        assert_eq!(
            main(
                &[config.display().to_string()],
                &mut &input[..],
                &mut out,
                &mut err
            ),
            0
        );
        assert!(
            serde_json::from_slice::<serde_json::Value>(&out).unwrap()["symphony-stall"]["load"]
                .as_str()
                .is_some_and(|s| !s.is_empty())
        );
        assert_eq!(
            main(
                &[config.display().to_string()],
                &mut &input[..],
                &mut Broken,
                &mut Vec::new()
            ),
            2
        );
        struct BadInput;
        impl std::io::Read for BadInput {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("broken input"))
            }
        }
        impl BufRead for BadInput {
            fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
                Err(std::io::Error::other("broken input"))
            }
            fn consume(&mut self, _: usize) {}
        }
        assert_eq!(
            main(
                &[config.display().to_string()],
                &mut BadInput,
                &mut Vec::new(),
                &mut Vec::new()
            ),
            2
        );
    }
}
