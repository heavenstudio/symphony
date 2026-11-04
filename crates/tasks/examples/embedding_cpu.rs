//! Offline CPU measurement from the frozen scratch snapshot; never opens a database.
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;
use symphony_board::embedding::{Bge, Embedder, text_hash};

fn cpu() -> std::io::Result<f64> {
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: the clock writes one initialized timespec for this process.
    if unsafe { libc::clock_gettime(libc::CLOCK_PROCESS_CPUTIME_ID, &mut time) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(time.tv_sec as f64 + time.tv_nsec as f64 / 1e9)
}

fn load() -> String {
    std::process::Command::new("uptime")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|e| e.to_string())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let arg = |name| {
        args.windows(2)
            .find(|a| a[0] == name)
            .map(|a| a[1].as_str())
    };
    let snapshot = arg("--snapshot").ok_or("--snapshot FILE is required")?;
    let out = PathBuf::from(arg("--out").ok_or("--out DIR is required")?);
    let threads = arg("--threads").unwrap_or("1").parse::<usize>()?;
    let samples = arg("--samples").unwrap_or("128").parse::<usize>()?;
    if samples == 0 {
        return Err("--samples must be positive".into());
    }
    let dump: symphony_board::rows::Dump = serde_json::from_slice(&fs::read(snapshot)?)?;
    fs::create_dir_all(&out)?;
    let start = Instant::now();
    let before = cpu()?;
    let common =
        symphony_board::git::common_dir(&std::env::current_dir()?).map_err(|e| e.to_string())?;
    let mut model = Bge::load_with_threads(&common, threads)?;
    let mut report = json!({"threads":threads,"cores":std::thread::available_parallelism()?.get(),
        "load_before":load(),"model_load_wall_s":start.elapsed().as_secs_f64(),"model_load_cpu_s":cpu()?-before});
    let start = Instant::now();
    let before = cpu()?;
    let mut chunks = Vec::new();
    let mut one_task = Vec::new();
    let mut sources = 0;
    for task in &dump.tasks {
        let parts = model.chunks(&format!("{}\n{}", task.title, task.body))?;
        if task.slug == "tools/verify-wall-budget" {
            one_task.extend(parts.clone());
        }
        chunks.extend(parts);
        sources += 1;
    }
    for message in dump.messages.iter().filter(|m| m.kind != "system") {
        let parts = model.chunks(&message.text)?;
        if message.slug == "tools/verify-wall-budget" {
            one_task.extend(parts.clone());
        }
        chunks.extend(parts);
        sources += 1;
    }
    chunks.sort_by_key(|c| c.tokens.len());
    one_task.sort_by_key(|c| c.tokens.len());
    report["tokenization"] = json!({"cpu_s":cpu()?-before,"wall_s":start.elapsed().as_secs_f64(),"sources":sources,"chunks":chunks.len()});
    let batches: Vec<_> = chunks.chunks(8).collect();
    if batches.is_empty() {
        return Err("snapshot has no chunks".into());
    }
    let selected = sampled_batches(batches.len(), samples);
    let samples = selected.len();
    // Warm the shortest and longest shape outside the timed sample.
    model.embed(batches[0])?;
    model.embed(batches[batches.len() - 1])?;
    let mut measured = Vec::new();
    let mut vectors = Vec::new();
    let mut estimated_cpu = 0.0;
    for (i, (index, weight)) in selected.into_iter().enumerate() {
        let batch = batches[index];
        let start = Instant::now();
        let before = cpu()?;
        let output = model.embed(batch)?;
        let seconds = cpu()? - before;
        let wall = start.elapsed().as_secs_f64();
        estimated_cpu += seconds * weight as f64;
        for vector in output {
            for value in vector {
                vectors.extend_from_slice(&value.to_le_bytes());
            }
        }
        measured.push(json!({"index":index,"represents":weight,"chunks":batch.len(),"max_tokens":batch.iter().map(|c|c.tokens.len()).max(),"cpu_s":seconds,"wall_s":wall}));
        if (i + 1) % 16 == 0 {
            println!("sample {}/{samples}", i + 1);
        }
    }
    let start = Instant::now();
    let before = cpu()?;
    for batch in one_task.chunks(8) {
        model.embed(batch)?;
    }
    report["one_task_inference"] = json!({"slug":"tools/verify-wall-budget","chunks":one_task.len(),"cpu_s":cpu()?-before,"wall_s":start.elapsed().as_secs_f64()});
    if let Some(reference) = arg("--reference") {
        let previous = fs::read(reference)?;
        if previous.len() != vectors.len() {
            return Err("reference vector length differs".into());
        }
        let max_abs_diff = previous
            .as_chunks::<4>()
            .0
            .iter()
            .zip(vectors.as_chunks::<4>().0)
            .map(|(a, b)| (f32::from_le_bytes(*a) - f32::from_le_bytes(*b)).abs())
            .fold(0.0_f32, f32::max);
        report["reference"] =
            json!({"bit_identical":previous==vectors,"max_abs_diff":max_abs_diff});
    }
    fs::write(out.join("vectors.f32"), &vectors)?;
    // The hash labels the snapshot; it never substitutes for the recorded timing inputs.
    report["snapshot_hash"] = json!(text_hash(&fs::read_to_string(snapshot)?));
    report["estimated_full_inference_cpu_s"] = json!(estimated_cpu);
    report["samples"] = json!(measured);
    report["load_after"] = json!(load());
    fs::write(out.join("report.json"), serde_json::to_vec_pretty(&report)?)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn sampled_batches(total: usize, samples: usize) -> Vec<(usize, usize)> {
    let samples = samples.min(total);
    (0..samples)
        .map(|i| {
            let first = i * total / samples;
            let end = (i + 1) * total / samples;
            ((first + end - 1) / 2, end - first)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stratified_sample_weights_cover_every_batch_without_duplicate_representatives() {
        assert_eq!(sampled_batches(0, 128), Vec::new());
        assert_eq!(sampled_batches(3, 128), vec![(0, 1), (1, 1), (2, 1)]);
        assert_eq!(sampled_batches(11, 3), vec![(1, 3), (4, 4), (8, 4)]);
        let samples = sampled_batches(3475, 128);
        assert_eq!(samples.len(), 128);
        assert_eq!(
            samples.iter().map(|(_, weight)| weight).sum::<usize>(),
            3475
        );
        assert!(samples.windows(2).all(|pair| pair[0].0 < pair[1].0));
        assert!(
            samples
                .iter()
                .all(|(index, weight)| *index < 3475 && (27..=28).contains(weight))
        );
    }
}
