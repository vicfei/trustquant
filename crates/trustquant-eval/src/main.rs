//! trustquant-eval: deterministic trace eval runner (ADR-0006 gate 6).
//! Model-explanation evals are defined under evals/explain and executed only
//! by the periodic model layer — never in CI (ADR-0004 of v1, carried over).

mod runner;

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("run") => {
            let dir = args
                .iter()
                .position(|a| a == "--cases")
                .and_then(|i| args.get(i + 1))
                .map(std::path::PathBuf::from)
                .unwrap_or_else(runner::default_cases_dir);
            let report = runner::run_cases_dir(&dir);
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
            if report.failed == 0 {
                std::process::ExitCode::SUCCESS
            } else {
                std::process::ExitCode::FAILURE
            }
        }
        Some("explain") => {
            let dir = runner::workspace_root().join("evals/explain");
            let mut listed = 0;
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for e in entries
                    .flatten()
                    .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
                {
                    if let Ok(text) = std::fs::read_to_string(e.path())
                        && let Ok(v) = serde_json::from_str::<serde_json::Value>(&text)
                    {
                        println!(
                            "[experimental] {} — {}",
                            v["id"].as_str().unwrap_or("?"),
                            v["task"].as_str().unwrap_or("?")
                        );
                        listed += 1;
                    }
                }
            }
            println!("{listed} explanation eval case(s); run via the periodic model layer only");
            std::process::ExitCode::SUCCESS
        }
        _ => {
            eprintln!("usage: trustquant-eval <run|explain> [--cases <dir>]");
            std::process::ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::runner;

    #[test]
    fn all_deterministic_cases_pass() {
        let report = runner::run_cases_dir(&runner::default_cases_dir());
        for c in &report.cases {
            if !c.passed {
                for s in &c.steps {
                    if let Some(f) = &s.failure {
                        eprintln!("{} step {}: {}", c.case_id, s.step, f);
                    }
                }
            }
        }
        assert_eq!(
            report.failed, 0,
            "{}/{} cases failed",
            report.failed, report.total
        );
        assert!(report.total >= 5, "expected at least 5 cases");
    }
}
