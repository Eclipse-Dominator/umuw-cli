use anyhow::{Result, bail};

use crate::{
    commands::run::LaunchCtx,
    config::{self, Game},
    dmenu,
};

pub fn open_menu_selector<D: dmenu::DMenuAdapter>(
    ctx: &LaunchCtx<'_, D>,
) -> Result<Option<String>> {
    let all_games = config::load_all::<Game>(ctx.config_root)?;
    let names: Vec<&str> = all_games.keys().map(|name| (*name).as_ref()).collect();
    let res = ctx.dmenu.choose("Launch game", &names, 0)?;
    match res {
        dmenu::Pick::Selected(name) => Ok(Some(name)),
        dmenu::Pick::Cancel => Ok(None),
        dmenu::Pick::Timeout => bail!("Timeout should NEVER be reached in menu"),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        commands::run::{self},
        config::SaveableToml,
        dmenu::tests_util::{CancelAdapter, FirstChoiceAdapter},
    };

    use super::*;

    #[test]
    fn open_menu_selector_handles_cancel() {
        let game_names = vec!["game1", "game 2 demo", "app1", "app3"];

        let t_ctx = run::tests_util::TestCtx::new(&["proton_names"]);
        let g = Game::default();
        for game in game_names {
            g.save(t_ctx.config_root.path(), game).unwrap();
        }
        let ctx = t_ctx.ctx(&CancelAdapter, &[]);
        assert_eq!(open_menu_selector(&ctx).unwrap(), None);
    }

    #[test]
    fn open_menu_selector_handles_select() {
        let mut game_names = vec!["game1", "game 2 demo", "app1", "app3"];
        let t_ctx = run::tests_util::TestCtx::new(&["proton_names"]);
        let g = Game::default();
        for game in &game_names {
            g.save(t_ctx.config_root.path(), game).unwrap();
        }
        let ctx = t_ctx.ctx(&FirstChoiceAdapter, &[]);
        game_names.sort();
        assert_eq!(
            open_menu_selector(&ctx).unwrap(),
            Some(game_names.remove(0).to_string())
        );
    }
}
