mod commands;
mod config;
mod dmenu;
mod gamescope;
mod paths;
mod prefix;

use std::process;

use anyhow::{Context, bail};
use clap::{Parser, Subcommand};

use crate::{
    commands::{
        install::{self, InstallResult},
        list::list_profiles,
        new::ConfigStatus,
        run::{LaunchCtx, dry_run, run},
        sync::{self, SyncReport},
    },
    config::{FallbackPath, Group, SaveableToml},
    paths::{get_home, initialize_config},
    prefix::prefix_path,
};

#[derive(Parser)]
#[command(name = "umuw", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum ConfigSubcommand {
    /// edit/reconfigure an existing group
    Group {
        /// name of the group
        name: String,
        /// rename the group
        #[arg(long = "rename")]
        rename: Option<String>,
    },
    /// edit/reconfigure an existing game
    Game {
        /// name of the game
        name: String,
        /// rename the game
        #[arg(long = "rename")]
        rename: Option<String>,
    },
    /// edit/reconfigure an the root configuration for umuw
    General {},
}

#[derive(Subcommand)]
enum NewSubcommand {
    /// create a new entry for the given exe
    Exe {
        /// entry point of the executable
        exe: String,

        /// name of the entry, will use file stem if left empty
        #[arg(long = "name")]
        name: Option<String>,

        /// create a new group for this executable
        #[arg(long = "new-grp", conflicts_with = "use_group")]
        new_group: Option<String>,

        /// reuse an existing group for this executable
        #[arg(long = "use-grp")]
        use_group: Option<String>,
    },
    /// create an new group
    Group {
        /// name of the group to be added
        name: String,
    },
}

#[derive(Subcommand)]
enum Command {
    /// run an installer exe into a new/reused/default prefix group
    Install {
        installer: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,

        #[arg(long = "new-grp", conflicts_with = "use_group")]
        new_group: Option<String>,

        #[arg(long = "use-grp")]
        use_group: Option<String>,
    },

    /// register or tune an .exe against a group's prefix
    Config {
        #[command(subcommand)]
        action: ConfigSubcommand,
    },

    New {
        #[command(subcommand)]
        action: NewSubcommand,
    },

    /// Launch a game/application that is stored by umuw
    Run {
        target: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
        #[arg(long)]
        group: Option<String>,
        #[arg(long = "dry-run")]
        is_dry_run: bool,
    },
    /// Sync profiles with the .desktop profiles
    Sync,
    Menu,
    List,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // this is only rlly used for linux since umu-run is linux only
    let proj_dir =
        directories::ProjectDirs::from("", "", "umuw").context("locating umuw config dir")?;
    let app_dir = directories::BaseDirs::new().context("obtaining base dirs")?;
    let local_app_root = app_dir.data_local_dir().join("applications/umuw/");
    let icon_root = app_dir.data_local_dir().join("icons/custom-icons/umuw/");
    let config_root = proj_dir.config_dir();
    let home = &get_home()?;
    let proton_search_folders = paths::get_proton_origins();
    let default_setting = config::Setting::get_user_default(config_root)?;
    let dmenu = default_setting.dmenu;
    let editor = default_setting.editor;
    let gs_passthrough_env = gamescope::collect_passthrough_env();
    let ctx = LaunchCtx {
        config_root,
        dmenu: &dmenu,
        gs_passthrough_env: &gs_passthrough_env,
        home,
        proton_search_folders: &proton_search_folders,
        icon_root: &icon_root,
        local_app_root: &local_app_root,
        editor: editor.as_deref(),
    };
    initialize_config(config_root)?;

    match cli.command {
        Command::Install {
            installer,
            args,
            new_group: as_group,
            use_group: reuse_group,
            ..
        } => {
            let group = as_group
                .as_deref()
                .map(|g| (g, true))
                .or_else(|| reuse_group.as_deref().map(|g| (g, false)));

            match install::install(ctx, &installer, group, args)? {
                InstallResult::GroupAlrExist(name) => {
                    println!("Group {name} already exist, consider --reuse instead")
                }
                InstallResult::GroupNotFound(name) => {
                    println!("Group {name} does not exist, consider --as to create a new group")
                }
                InstallResult::InstallRun(code) => {
                    println!("Installation exited with {}", code);
                    process::exit(code);
                }
            }
        }
        Command::Config { action } => {
            match action {
                ConfigSubcommand::Game { name, rename } => {
                    let game_name = match rename.as_deref() {
                        Some(n) => config::Game::rename(config_root, &name, n)?,
                        None => &name,
                    };
                    config::Game::edit(config_root, game_name, ctx.editor)?
                }
                ConfigSubcommand::Group { name, rename } => {
                    let group_name = match rename.as_deref() {
                        Some(to) => {
                            Group::rename(config_root, &name, to)?;
                            match config::update_group_members(ctx.config_root, &name, to)? {
                                config::UpdateGroupMembersRes::Fail((good, total)) => {
                                    println!("{good}/{total} relevant games updated!")
                                }
                                config::UpdateGroupMembersRes::Success(total) => {
                                    println!("{total} games updated!")
                                }
                            }
                            match prefix::transfer_prefix(&name, to, ctx.home)? {
                                prefix::TransferPrefixRes::SrcMissing
                                | prefix::TransferPrefixRes::Success => {}
                                prefix::TransferPrefixRes::TargetExist => {
                                    eprintln!(
                                        "prefix {} already exist, unable to rename prefix",
                                        prefix_path(to, ctx.home).display()
                                    )
                                }
                            }
                            to
                        }
                        None => &name,
                    };
                    config::Group::edit(config_root, group_name, ctx.editor)?
                }
                ConfigSubcommand::General {} => {
                    config::Global::edit(config_root, config::UmuwConfig::DEFAULT_CONFIG, None)?
                }
            };
        }
        Command::New { action } => match action {
            NewSubcommand::Exe {
                exe,
                name,
                new_group,
                use_group,
            } => {
                let group = new_group
                    .as_deref()
                    .map(|g| (g, true))
                    .or_else(|| use_group.as_deref().map(|g| (g, false)));
                match commands::new::config(config_root, exe, group, name)? {
                    ConfigStatus::InvalidPath(s) => {
                        println!("executable {} is invalid", s.display())
                    }
                    ConfigStatus::GroupAlrExist(name) => {
                        println!("Group {name} already exist, consider --reuse instead")
                    }
                    ConfigStatus::GroupNotFound(name) => {
                        println!("Group {name} does not exist, consider --as to create a new group")
                    }
                    ConfigStatus::AppExist(s) => println!("App with name {s} already exists"),
                    ConfigStatus::Success(s) => {
                        println!("App has been added successfully, launch with umuw run {s}.")
                    }
                }
            }
            NewSubcommand::Group { name } => match Group::ensure(ctx.config_root, &name, true)? {
                config::EnsureOutcome::AlrExist(name) => println!("Group {name} already exist!"),
                config::EnsureOutcome::NotFound(name) => bail!("UNREACHABLE: GRP {name} NOT FOUND"),
                config::EnsureOutcome::Ensured(name) => println!("Group {name} has been created"),
            },
        },

        Command::Run {
            target,
            args,
            is_dry_run,
            group,
            ..
        } => {
            if is_dry_run {
                println!(
                    "Expected launch:\n{}",
                    dry_run(&ctx, &target, group.as_deref(), args)?
                )
            } else {
                let code = run(&ctx, &target, group.as_deref(), args)?;
                println!("{} exited with {}", target, code);
                process::exit(code);
            }
        }
        Command::Sync => {
            match sync::sync(ctx.config_root, ctx.local_app_root, ctx.icon_root)? {
                SyncReport::Success(count) => println!("({count}) app sync succesfully"),
                SyncReport::WithFailure((success, total)) => {
                    println!("({success}/{total}) app sync succesfully")
                }
            };
            sync::refresh_desktop_entries(ctx.local_app_root);
        }
        Command::Menu => {
            if let Some(game) = commands::menu::open_menu_selector(&ctx)? {
                let code = run(&ctx, &game, None, vec![])?;
                println!("{} exited with {}", game, code);
                process::exit(code);
            }
        }
        Command::List => println!("{}", list_profiles(config_root, home)?),
    }

    Ok(())
}
