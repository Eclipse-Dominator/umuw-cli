use std::{io::Write, process::Stdio};

use std::process::{self, Command};

use anyhow::{Context, Result};

#[derive(Clone, PartialEq, Debug)]
pub enum Pick {
    Selected(String),
    Timeout,
    Cancel,
}

pub trait DMenuAdapter {
    fn choose(&self, prompt: &str, options: &[&str], timeout_secs: u64) -> Result<Pick>;
}

#[derive(Default)]
pub struct NoctaliaAdapter;

#[derive(Default)]

pub struct RofiAdapter;

impl DMenuAdapter for NoctaliaAdapter {
    fn choose(&self, prompt: &str, options: &[&str], timeout_secs: u64) -> Result<Pick> {
        let timeout_str = timeout_secs.to_string();
        let mut args = vec!["timeout", timeout_str.as_str()];
        if timeout_secs == 0 {
            args.clear();
        }
        args.append(&mut vec!["noctalia", "dmenu", "-p", prompt]);
        let prog = args.remove(0);
        let mut noc_dmenu = process::Command::new(prog)
            .args(args)
            .env_remove("LD_LIBRARY_PATH")
            .env_remove("LD_PRELOAD")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .context("Launching noctalia dmenu")?;

        {
            noc_dmenu
                .stdin
                .take()
                .context("writing to stdin")?
                .write_all(options.join("\n").as_bytes())?;
        }

        let out = noc_dmenu.wait_with_output().context("obtaining choices")?;
        if out.status.code() == Some(124) {
            // timeout occured
            let res = Command::new("noctalia")
                .args(["msg", "panel-close", "launcher"])
                .env_remove("LD_LIBRARY_PATH")
                .env_remove("LD_PRELOAD")
                .spawn()
                .and_then(|mut child| child.wait());

            if let Err(e) = res {
                eprintln!("Warning: error occured while closing noctalia dmenu: {e:#}");
            }

            return Ok(Pick::Timeout);
        }

        let choice = String::from_utf8_lossy(&out.stdout).trim_end().to_string();

        if !out.status.success() || choice.is_empty() {
            return Ok(Pick::Cancel);
        }
        Ok(Pick::Selected(choice))
    }
}

impl DMenuAdapter for RofiAdapter {
    fn choose(&self, prompt: &str, options: &[&str], timeout_secs: u64) -> Result<Pick> {
        let timeout_str = timeout_secs.to_string();
        let mut args = vec!["timeout", timeout_str.as_str()];
        if timeout_secs == 0 {
            args.clear();
        }
        args.append(&mut vec!["rofi", "-dmenu", "-p", prompt]);
        let prog = args.remove(0);
        let mut rofi_dmenu = process::Command::new(prog)
            .args(args)
            .env_remove("LD_LIBRARY_PATH")
            .env_remove("LD_PRELOAD")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .context("Launching rofi dmenu")?;

        {
            rofi_dmenu
                .stdin
                .take()
                .context("writing to stdin")?
                .write_all(options.join("\n").as_bytes())?;
        }

        let out = rofi_dmenu.wait_with_output().context("obtaining choices")?;
        if out.status.code() == Some(124) {
            return Ok(Pick::Timeout);
        }

        let choice = String::from_utf8_lossy(&out.stdout).trim_end().to_string();

        if !out.status.success() || choice.is_empty() {
            return Ok(Pick::Cancel);
        }
        Ok(Pick::Selected(choice))
    }
}

#[cfg(test)]
pub mod tests_util {
    use super::*;

    pub struct TimeoutAdapter;

    pub struct CancelAdapter;

    pub struct FirstChoiceAdapter;

    pub struct SecondChoiceAdapter;

    impl DMenuAdapter for TimeoutAdapter {
        fn choose(&self, _prompt: &str, _options: &[&str], _timeout_secs: u64) -> Result<Pick> {
            Ok(Pick::Timeout)
        }
    }

    impl DMenuAdapter for CancelAdapter {
        fn choose(&self, _prompt: &str, _options: &[&str], _timeout_secs: u64) -> Result<Pick> {
            Ok(Pick::Cancel)
        }
    }

    impl DMenuAdapter for FirstChoiceAdapter {
        fn choose(&self, _prompt: &str, options: &[&str], _timeout_secs: u64) -> Result<Pick> {
            let chosen = options.first().unwrap().to_owned();
            Ok(Pick::Selected(chosen.into()))
        }
    }

    impl DMenuAdapter for SecondChoiceAdapter {
        fn choose(&self, _prompt: &str, options: &[&str], _timeout_secs: u64) -> Result<Pick> {
            let chosen = options.get(1).unwrap().to_owned();
            Ok(Pick::Selected(chosen.into()))
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    fn default_opt() -> Vec<&'static str> {
        vec!["opt1", "opt2", "opt3", "opt4"]
    }

    #[test]
    #[ignore = "Requires human interaction with noctalia"]
    fn noctalia_adaptor_timeout_correctly() {
        let res = NoctaliaAdapter
            .choose("TIMEOUT test", &default_opt(), 1)
            .unwrap();
        assert_eq!(res, Pick::Timeout);
    }

    #[test]
    #[ignore = "Requires human interaction with noctalia"]
    fn noctalia_adaptor_cancel_correctly() {
        let res = NoctaliaAdapter
            .choose("CANCEL via esc test", &default_opt(), 20)
            .unwrap();
        assert_eq!(res, Pick::Cancel);
    }

    #[test]
    #[ignore = "Requires human interaction with noctalia"]
    fn noctalia_adaptor_select_correctly() {
        let res = NoctaliaAdapter
            .choose("SELECT test (select first opt)", &default_opt(), 20)
            .unwrap();
        assert_eq!(res, Pick::Selected(default_opt().swap_remove(0).into()));
    }

    #[test]
    #[ignore = "Requires human interaction with rofi"]
    fn rofi_adaptor_timeout_correctly() {
        let res = RofiAdapter
            .choose("TIMEOUT test", &default_opt(), 1)
            .unwrap();
        assert_eq!(res, Pick::Timeout);
    }

    #[test]
    #[ignore = "Requires human interaction with rofi"]
    fn rofi_adaptor_cancel_correctly() {
        let res = RofiAdapter
            .choose("CANCEL via esc test", &default_opt(), 20)
            .unwrap();
        assert_eq!(res, Pick::Cancel);
    }

    #[test]
    #[ignore = "Requires human interaction with rofi"]
    fn rofi_adaptor_select_correctly() {
        let res = RofiAdapter
            .choose("SELECT test (select first opt)", &default_opt(), 20)
            .unwrap();
        assert_eq!(res, Pick::Selected(default_opt().swap_remove(0).into()));
    }
}
