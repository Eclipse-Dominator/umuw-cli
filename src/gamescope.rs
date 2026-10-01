use std::{
    any::type_name,
    env,
    ffi::OsString,
    fmt::{Display, Formatter},
    process::Command,
    str::FromStr,
    vec,
};

use anyhow::{Context, Result, bail};

use crate::{
    config::{self, GamescopeMode},
    dmenu::{DMenuAdapter, Pick},
};

const PROMPT_TIMEOUT: u64 = 10;
const PASSTHROUGH_ENV: &[&str] = &["OBS_VKCAPTURE"];

#[derive(PartialEq, Debug)]
pub struct GamescopeSpec {
    w_in: u32,
    w_out: u32,
    h_in: u32,
    h_out: u32,
    fps: u32,
    hdr: bool,
    vrr: bool,
    grab: bool,
}

pub enum PickerOutcome {
    Selected(usize),
    Direct,
    Cancel,
    Timeout,
}

#[derive(PartialEq, Debug)]
pub enum PromptOutcome {
    Gamescope(GamescopeSpec),
    Native,
    Cancelled,
}

impl Default for GamescopeSpec {
    fn default() -> Self {
        GamescopeSpec {
            w_in: 3840,
            w_out: 3840,
            h_in: 2160,
            h_out: 2160,
            fps: 120,
            hdr: false,
            vrr: false,
            grab: false,
        }
    }
}

impl Display for GamescopeSpec {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{}x{}] => {}x{}@{}{}{}{}",
            self.w_in,
            self.h_in,
            self.w_out,
            self.h_out,
            self.fps,
            if self.hdr { " HDR" } else { "" },
            if self.vrr { " VRR" } else { "" },
            if self.grab { " grab" } else { "" },
        )
    }
}

pub fn create_fallback_gamescope_spec(eff: &config::Effective) -> GamescopeSpec {
    let hdr = eff.hdr.unwrap_or_default();
    GamescopeSpec {
        hdr,
        ..Default::default()
    }
}

pub fn create_gamescope_cmd(
    spec: &GamescopeSpec,
    gs_passthrough_env: &[(&str, Option<OsString>)],
) -> std::process::Command {
    let mut cmd = std::process::Command::new("gamescope");
    let args = gamescope_args(spec);
    let env_args = gamescope_env_handler(&mut cmd, spec, gs_passthrough_env);

    cmd.args(args).args(["--", "env"]).args(env_args);
    cmd
}

pub fn obtain_gamescope_spec(
    dmenu: &impl DMenuAdapter,
    opts: &[String],
    mode: GamescopeMode,
    fallback_opt: &GamescopeSpec,
) -> Result<PromptOutcome> {
    let mut specs: Vec<GamescopeSpec> = vec![];
    for opt in opts {
        specs.push(parse_opt(opt, fallback_opt)?);
    }

    let grab_nth_ele = |mut specs: Vec<GamescopeSpec>, nth| {
        if nth >= specs.len() {
            bail!(
                "Index out of bound: getting {} out of specs with {} element",
                nth,
                specs.len()
            );
        }
        Ok(PromptOutcome::Gamescope(specs.swap_remove(nth)))
    };

    match mode {
        GamescopeMode::Native => Ok(PromptOutcome::Native),
        GamescopeMode::Gamescope => grab_nth_ele(specs, 0),
        GamescopeMode::Prompt => match prompt(dmenu, &specs, PROMPT_TIMEOUT)? {
            PickerOutcome::Cancel => Ok(PromptOutcome::Cancelled),
            PickerOutcome::Direct => Ok(PromptOutcome::Native),
            PickerOutcome::Timeout => grab_nth_ele(specs, 0),
            PickerOutcome::Selected(index) => grab_nth_ele(specs, index),
        },
    }
}

pub fn collect_passthrough_env() -> Vec<(&'static str, Option<OsString>)> {
    // collects env var for list of variable to be ignored by gamescope but picked up by the inner method
    let mut ret = vec![];
    for test_env in PASSTHROUGH_ENV {
        ret.push((*test_env, env::var_os(test_env)));
    }
    ret
}

fn to<T>(var: Option<&str>, var_name: &str) -> Result<T>
where
    T: FromStr,
    T::Err: std::error::Error + Send + Sync + 'static,
{
    var.with_context(|| format!("obtaining {}", var_name))
        .and_then(|val| {
            val.parse::<T>()
                .with_context(|| format!("converting {} -> {}", val, type_name::<T>()))
        })
}

fn parse_opt(spec: &str, fallback_opt: &GamescopeSpec) -> Result<GamescopeSpec> {
    // fallback_opt provides fallback for optional fields
    let mut tokens = spec.split_whitespace();
    let w_in = to::<u32>(tokens.next(), "w")?;
    let h_in = to::<u32>(tokens.next(), "h")?;
    let w_out = to::<u32>(tokens.next(), "W")?;
    let h_out = to::<u32>(tokens.next(), "H")?;
    let fps = to::<u32>(tokens.next(), "fps")?;
    let mut hdr = fallback_opt.hdr;
    let mut vrr = fallback_opt.vrr;
    let mut grab = fallback_opt.grab;

    for token in tokens {
        match token {
            t if t.eq_ignore_ascii_case("hdr") => hdr = true,
            t if t.eq_ignore_ascii_case("vrr") => vrr = true,
            t if t.eq_ignore_ascii_case("grab") => grab = true,
            other => bail!("Unknown gamescope token {other} in {spec}"),
        }
    }

    Ok(GamescopeSpec {
        w_in,
        h_in,
        w_out,
        h_out,
        fps,
        hdr,
        vrr,
        grab,
    })
}

fn gamescope_args(spec: &GamescopeSpec) -> Vec<OsString> {
    let mut ret: Vec<OsString> = vec![];
    for (k, v) in [
        ("-W", spec.w_out),
        ("-H", spec.h_out),
        ("-w", spec.w_in),
        ("-h", spec.h_in),
        ("-r", spec.fps),
    ] {
        ret.push(k.into());
        ret.push(v.to_string().into());
    }
    if spec.hdr {
        ret.push(OsString::from("--hdr-enabled"));
    }
    if spec.vrr {
        ret.push(OsString::from("--adaptive-sync"));
    }
    if spec.grab {
        ret.push(OsString::from("--grab"));
    }
    ret.push(OsString::from("--xwayland-count"));
    ret.push(OsString::from("2"));
    ret.push(OsString::from("--virtual-connector-strategy"));
    ret.push(OsString::from("PerWindow"));
    ret
}

fn gamescope_env_handler(
    cmd: &mut Command,
    spec: &GamescopeSpec,
    avoided_envs: &[(&str, Option<OsString>)],
) -> Vec<OsString> {
    /*
       env that impacts gamescope and should be disabled and re-enabled based on certain settings
       DXVK_HDR                     | enables hdr when set to 1
       ENABLE_GAMESCOPE_WSI         |
       PROTON_ENABLE_WAYLAND        | gamescope prefers xwayland over wayland

       avoided_envs contains a list of env to be avoided passing to gamescope but passed down to interior calls
       such as:
       OBS_VKCAPTURE                | should ignore gamescope, and reenable if set later

       then returns the list of env to be added inside of gamescope args (-- env [args] ... umu-run)
    */
    cmd.env_remove("DXVK_HDR")
        .env_remove("ENABLE_GAMESCOPE_WSI")
        .env_remove("PROTON_ENABLE_WAYLAND");

    for (k, v) in avoided_envs {
        if v.is_some() {
            cmd.env_remove(k);
        }
    }

    let mut ret = vec![OsString::from("ENABLE_GAMESCOPE_WSI=1")];

    if spec.hdr {
        ret.push(OsString::from("DXVK_HDR=1"));
    }

    for (k, v) in avoided_envs {
        if let Some(val) = v {
            let mut env_to_add = OsString::from(format!("{}=", k));
            env_to_add.push(val);
            ret.push(env_to_add);
        }
    }

    ret
}

fn prompt(
    dmenu: &impl DMenuAdapter,
    choices: &[GamescopeSpec],
    timeout_secs: u64,
) -> Result<PickerOutcome> {
    let _choices_str = choices
        .iter()
        .map(|spec| spec.to_string())
        .collect::<Vec<String>>();
    let mut prompt_options = vec![];
    for choice_str in &_choices_str {
        prompt_options.push(choice_str.as_str());
    }
    prompt_options.push("Direct");
    prompt_options.push("Cancel");

    let res = dmenu.choose("Choose launch option", &prompt_options, timeout_secs)?;
    let ret = match res {
        Pick::Timeout => PickerOutcome::Timeout,
        Pick::Cancel => PickerOutcome::Cancel,
        Pick::Selected(ref s) if s == "Direct" => PickerOutcome::Direct,
        Pick::Selected(ref s) if s == "Cancel" => PickerOutcome::Cancel,
        Pick::Selected(s) => {
            let index = prompt_options.iter().position(|o| *o == s);
            match index {
                Some(outcome) => PickerOutcome::Selected(outcome),
                None => bail!("selected option not found in {}", prompt_options.join(", ")),
            }
        }
    };

    Ok(ret)
}

#[cfg(test)]
mod tests {
    use crate::dmenu::{
        NoctaliaAdapter,
        tests_util::{CancelAdapter, FirstChoiceAdapter, SecondChoiceAdapter, TimeoutAdapter},
    };

    use super::*;

    #[test]
    fn gamescopespec_to_string_is_correct() {
        let test = GamescopeSpec {
            fps: 23,
            h_in: 123,
            h_out: 123,
            w_in: 1233,
            w_out: 1233,
            vrr: true,
            ..Default::default()
        };
        assert_eq!(test.to_string(), "[1233x123] => 1233x123@23 VRR");
    }

    #[test]
    fn to_converts_string_to_u32() {
        assert_eq!(to::<u32>(Some("1234"), "some name").unwrap(), 1234);
    }

    #[test]
    fn to_panics_on_invalid_conversions() {
        to::<u32>(Some("1234e"), "some name").unwrap_err();
    }

    #[test]
    fn to_panics_on_none() {
        to::<u32>(None, "some name").unwrap_err();
    }

    #[test]
    fn parse_opt_parses_base_values() {
        let res = parse_opt("120 120 123123 123123 33", &GamescopeSpec::default()).unwrap();
        let expected = GamescopeSpec {
            w_in: 120,
            h_in: 120,
            w_out: 123123,
            h_out: 123123,
            fps: 33,
            ..Default::default()
        };
        assert_eq!(res, expected);
    }

    #[test]
    fn parse_opt_parses_optional_values() {
        let res = parse_opt("120 120 4000 2000 33 vrr hdr", &GamescopeSpec::default()).unwrap();
        let expected = GamescopeSpec {
            w_in: 120,
            h_in: 120,
            w_out: 4000,
            h_out: 2000,
            fps: 33,
            hdr: true,
            vrr: true,
            ..GamescopeSpec::default()
        };
        assert_eq!(res, expected);
    }

    #[test]
    fn parse_opt_panics_on_unknown_values() {
        let res = parse_opt("120 120 4000 2000 33 asdf hdr", &GamescopeSpec::default());
        assert!(
            res.unwrap_err()
                .to_string()
                .contains("Unknown gamescope token asdf")
        );
    }

    #[test]
    fn parse_opt_panics_on_missing_values() {
        let res = parse_opt("120 120", &GamescopeSpec::default());
        assert!(res.unwrap_err().to_string().contains("obtaining W"));
    }

    #[test]
    fn parse_opt_fallback_to_on_hdr() {
        let fallback = GamescopeSpec {
            hdr: true,
            ..Default::default()
        };
        let res = parse_opt("120 120 4000 2000 33 vrr", &fallback).unwrap();
        assert!(res.hdr);
    }

    #[test]
    fn create_gamescope_cmd_removes_expected_env() {
        let spec = GamescopeSpec {
            w_in: 120,
            h_in: 120,
            w_out: 4000,
            h_out: 2000,
            fps: 33,
            hdr: true,
            vrr: true,
            ..GamescopeSpec::default()
        };
        let cmd = create_gamescope_cmd(&spec, &[]);
        let mut rm_envs = vec![
            "DXVK_HDR".to_owned(),
            "ENABLE_GAMESCOPE_WSI".to_owned(),
            "PROTON_ENABLE_WAYLAND".to_owned(),
        ];
        rm_envs.sort();
        let mut res: Vec<String> = cmd
            .get_envs()
            .filter(|(_, v)| v.is_none())
            .map(|(k, _)| k.to_string_lossy().to_string())
            .collect();
        res.sort();
        assert_eq!(rm_envs, res);
    }

    #[test]
    fn create_gamescope_cmd_adds_expected_env() {
        let spec = GamescopeSpec {
            w_in: 120,
            h_in: 120,
            w_out: 4000,
            h_out: 2000,
            fps: 33,
            hdr: false,
            vrr: true,
            ..GamescopeSpec::default()
        };
        let cmd = create_gamescope_cmd(&spec, &[]);
        let expected_envs = vec!["ENABLE_GAMESCOPE_WSI".to_owned()];
        let res: Vec<String> = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();
        for expected_env in expected_envs {
            assert!(res.contains(&format!("{}={}", expected_env, 1)));
        }
    }

    #[test]
    fn create_gamescope_cmd_adds_expected_env_with_hdr() {
        let spec = GamescopeSpec {
            w_in: 120,
            h_in: 120,
            w_out: 4000,
            h_out: 2000,
            fps: 33,
            hdr: true,
            vrr: true,
            ..GamescopeSpec::default()
        };
        let cmd = create_gamescope_cmd(&spec, &[]);
        let expected_envs = vec!["ENABLE_GAMESCOPE_WSI".to_owned(), "DXVK_HDR".to_owned()];
        let res: Vec<String> = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();
        for expected_env in expected_envs {
            assert!(res.contains(&format!("{}={}", expected_env, 1)));
        }
    }

    #[test]
    fn create_gamescope_cmd_removes_and_adds_passthrough_var() {
        let gs_passthrough_env = [
            ("ASDF", Some(OsString::from("1"))),
            ("OBS", Some(OsString::from("1"))),
            ("None_existant", None),
        ];
        let expected_envs = [
            ("ASDF".to_owned(), Some(OsString::from("1"))),
            ("OBS".to_owned(), Some(OsString::from("1"))),
        ];
        let spec = GamescopeSpec {
            w_in: 120,
            h_in: 120,
            w_out: 4000,
            h_out: 2000,
            fps: 33,
            hdr: true,
            vrr: true,
            ..GamescopeSpec::default()
        };
        let cmd = create_gamescope_cmd(&spec, &gs_passthrough_env);
        let res: Vec<OsString> = cmd.get_args().map(|arg| arg.to_os_string()).collect();
        for (k, v) in expected_envs {
            let mut to_check = OsString::from(format!("{}=", k));
            to_check.push(v.unwrap());
            assert!(res.contains(&to_check));
        }
        assert!(!res.contains(&OsString::from("None_existant=")));
    }

    #[test]
    #[ignore = "Manual test: Requires interaction with NoctaliaAdapter/Noctalia dmenu"]
    fn prompt_manual_test() {
        let c = [
            GamescopeSpec::default(),
            GamescopeSpec {
                w_in: 12,
                h_in: 123,
                ..GamescopeSpec::default()
            },
            GamescopeSpec {
                w_in: 1280,
                h_in: 720,
                fps: 23,
                hdr: true,
                ..GamescopeSpec::default()
            },
        ];

        let _res = prompt(&NoctaliaAdapter, &c, 20).unwrap();
    }

    #[test]
    fn obtain_gamescope_spec_returns_cancel_when_cancel() {
        let res = obtain_gamescope_spec(
            &CancelAdapter,
            &config::default_gamescope_option(),
            GamescopeMode::Prompt,
            &GamescopeSpec::default(),
        )
        .unwrap();
        assert_eq!(res, PromptOutcome::Cancelled)
    }

    #[test]
    fn obtain_gamescope_spec_returns_first_option_when_timeout() {
        let opts = config::default_gamescope_option();

        let expected_opt = opts.first().unwrap();
        let res = obtain_gamescope_spec(
            &TimeoutAdapter,
            &opts,
            GamescopeMode::Prompt,
            &GamescopeSpec::default(),
        )
        .unwrap();

        assert_eq!(
            res,
            PromptOutcome::Gamescope(parse_opt(expected_opt, &GamescopeSpec::default()).unwrap())
        )
    }

    #[test]
    fn obtain_gamescope_spec_returns_1st_option_when_1st_choice_selected() {
        let opts = config::default_gamescope_option();

        let expected_opt = opts.first().unwrap();
        let res = obtain_gamescope_spec(
            &FirstChoiceAdapter,
            &opts,
            GamescopeMode::Prompt,
            &GamescopeSpec::default(),
        )
        .unwrap();

        assert_eq!(
            res,
            PromptOutcome::Gamescope(parse_opt(expected_opt, &GamescopeSpec::default()).unwrap())
        )
    }

    #[test]
    fn obtain_gamescope_spec_returns_2nd_option_when_2nd_choice_selected() {
        let opts = config::default_gamescope_option();

        let expected_opt = opts.get(1).unwrap();
        let res = obtain_gamescope_spec(
            &SecondChoiceAdapter,
            &opts,
            GamescopeMode::Prompt,
            &GamescopeSpec::default(),
        )
        .unwrap();

        assert_eq!(
            res,
            PromptOutcome::Gamescope(parse_opt(expected_opt, &GamescopeSpec::default()).unwrap())
        )
    }

    #[test]
    fn obtain_gamescope_spec_panics_when_gamescope_selected_with_empty_spec_list() {
        let res = obtain_gamescope_spec(
            &FirstChoiceAdapter,
            &[],
            GamescopeMode::Gamescope,
            &GamescopeSpec::default(),
        );
        assert!(res.unwrap_err().to_string().contains("Index out of bound"));
    }

    #[test]
    fn obtain_gamescope_spec_panics_when_timeout_on_empty_spec_list() {
        let res = obtain_gamescope_spec(
            &TimeoutAdapter,
            &[],
            GamescopeMode::Prompt,
            &GamescopeSpec::default(),
        );
        assert!(res.unwrap_err().to_string().contains("Index out of bound"));
    }

    #[test]
    fn obtain_gamescope_spec_returns_native_when_native_mode() {
        let res = obtain_gamescope_spec(
            &FirstChoiceAdapter,
            &[],
            GamescopeMode::Native,
            &GamescopeSpec::default(),
        );
        assert_eq!(res.unwrap(), PromptOutcome::Native)
    }

    #[test]
    fn obtain_gamescope_spec_returns_native_when_native_selected() {
        // prompts => [Direct, Cancel]
        let res = obtain_gamescope_spec(
            &FirstChoiceAdapter,
            &[],
            GamescopeMode::Prompt,
            &GamescopeSpec::default(),
        );
        assert_eq!(res.unwrap(), PromptOutcome::Native)
    }
}
