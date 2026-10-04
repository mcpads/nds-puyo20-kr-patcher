mod academy;
mod anti_piracy;
mod archive;
mod arm9;
mod art_import;
mod art_pixels;
mod art_review;
mod assets;
mod atlas;
mod battle_ui;
mod build;
mod buttons;
mod compress;
mod corpus;
mod font_compare;
mod fonts;
mod format;
mod graphics;
mod guide_obj;
mod guides;
mod hinting;
mod labels;
mod lc_font;
mod localize;
mod menu;
mod multiplayer;
mod nameplates;
mod panels;
mod pause;
mod pause_title;
mod point_get;
mod product;
mod puyo_hud;
mod records;
mod save;
mod screen_text;
mod screens;
mod selection;
mod sprites;
mod story;
mod system_dialogs;
mod text_objects;
mod titles;
mod toko;
mod unlock;
mod wifi_art;
mod wifi_dialog;
mod wifi_dialog_art;

#[cfg(test)]
mod test_input;

mod cli;
mod commands;

use anyhow::Result;
use clap::Parser;

fn main() -> Result<()> {
    let result = commands::run(cli::Cli::parse().command)?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
