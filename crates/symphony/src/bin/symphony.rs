//! `symphony status …` (`just symphony-status`), `symphony reply …` (`just symphony-reply`), `symphony ask …` (`just
//! symphony-ask`), `symphony shot …`, `symphony serve`.

use std::process::ExitCode;

fn main() -> ExitCode {
    // A `symphony shot`'s Chrome is owned by this binary run again, its leash, until the shot ends.
    if let Some(code) = chrome_devtools::leash_main() {
        return ExitCode::from(u8::try_from(code).unwrap_or(1));
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|command| command == "serve") {
        symphony::cli::end_with_parent();
    }
    let code = symphony::cli::main(
        &args,
        &mut std::io::stdin().lock(),
        &mut std::io::stdout().lock(),
        &mut std::io::stderr(),
    );
    ExitCode::from(u8::try_from(code).unwrap_or(2))
}
