mod build_rom;
mod prepare_text;

use crate::format::*;
use crate::{
    archive, arm9, assets, atlas, battle_ui, buttons, cli::Command, corpus, graphics, guide_obj,
    guides, localize, menu, nameplates, panels, pause, pause_title, records, screens, selection,
    story, titles,
};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}
fn profile(p: Option<PathBuf>) -> PathBuf {
    p.unwrap_or_else(|| root().join("config/source.json"))
}
fn new_json(p: &Path, v: &serde_json::Value) -> Result<()> {
    ensure!(!p.exists(), "output already exists");
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent)?;
    }
    assets::json_file(p, v)
}
pub(crate) fn run(command: Command) -> Result<Value> {
    Ok(match command {
        Command::CheckHangulNameBitmaps { rom, ram, out } => {
            let b = fs::read(rom)?;
            let r = crate::lc_font::check_bitmaps(&Rom::parse(&b)?, &fs::read(ram)?)?;
            new_json(&out, &r)?;
            r
        }
        Command::CheckNameFontLoaded {
            rom,
            ram,
            object,
            out,
        } => {
            let b = fs::read(rom)?;
            let r = crate::lc_font::check_loaded(&Rom::parse(&b)?, &fs::read(ram)?, object)?;
            new_json(&out, &r)?;
            r
        }
        Command::PrepareNameFont { source, font, out } => {
            let (b, _) = load(&source, &profile(None))?;
            crate::lc_font::hangul::prepare(&Rom::parse(&b)?, &font, &out)?
        }
        Command::InspectNameHangul { source, font, out } => {
            let (b, _) = load(&source, &profile(None))?;
            crate::lc_font::hangul::inspect(&Rom::parse(&b)?, &font, &out)?
        }
        Command::InspectLcFont { source, ram, out } => {
            let (b, _) = load(&source, &profile(None))?;
            let ram = ram.map(fs::read).transpose()?;
            crate::lc_font::inspect(&Rom::parse(&b)?, ram.as_deref(), &out)?
        }
        Command::CheckRecordsVram {
            rom,
            observation,
            out,
        } => {
            let b = fs::read(rom)?;
            let r = records::check_vram(&Rom::parse(&b)?, &fs::read(observation)?)?;
            new_json(&out, &r)?;
            r
        }
        Command::PrepareRecordLabels {
            source,
            translation,
            font,
            small_font,
            narrow_font,
            medium_font,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            records::prepare_labels(
                &Rom::parse(&b)?,
                &translation,
                &font,
                &small_font,
                narrow_font.as_deref(),
                medium_font.as_deref(),
                &out,
            )?
        }
        Command::PrepareRecordStatistics {
            source,
            translation,
            font,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            records::prepare_statistics(&Rom::parse(&b)?, &translation, &font, &out)?
        }
        Command::InspectRecordCaptions { source, out } => {
            let (b, _) = load(&source, &profile(None))?;
            records::inspect_captions(&Rom::parse(&b)?, &out)?
        }
        Command::InspectRecords { source, out } => {
            let (b, _) = load(&source, &profile(None))?;
            records::inspect(&Rom::parse(&b)?, &out)?
        }
        Command::PrepareRecords {
            source,
            translation,
            font,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            records::prepare(&Rom::parse(&b)?, &translation, &font, &out)?
        }
        Command::CheckPointPanelRam { rom, ram, out } => {
            let b = fs::read(rom)?;
            let r = crate::point_get::panel_residency(&Rom::parse(&b)?, &fs::read(ram)?)?;
            new_json(&out, &r)?;
            r
        }
        Command::CheckPointTitleRam { rom, ram, out } => {
            let b = fs::read(rom)?;
            let r = crate::point_get::residency(&Rom::parse(&b)?, &fs::read(ram)?)?;
            new_json(&out, &r)?;
            r
        }
        Command::CheckRecordsRam { rom, ram, out } => {
            let b = fs::read(rom)?;
            let r = records::residency(&Rom::parse(&b)?, &fs::read(ram)?)?;
            new_json(&out, &r)?;
            r
        }
        Command::CheckPauseTitleRam { rom, ram, out } => {
            let b = fs::read(rom)?;
            let r = pause_title::residency(&Rom::parse(&b)?, &fs::read(ram)?)?;
            new_json(&out, &r)?;
            r
        }
        Command::PreparePauseTitle {
            source,
            translation,
            artwork,
            graphics_only,
            font,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            pause_title::prepare(
                &Rom::parse(&b)?,
                &translation,
                artwork.as_deref(),
                graphics_only,
                &font,
                &out,
            )?
        }
        Command::CheckArm9Boot {
            rom,
            ram,
            observation,
            out,
        } => {
            let r = arm9::check_boot(&fs::read(rom)?, &fs::read(ram)?, &fs::read(observation)?)?;
            new_json(&out, &r)?;
            r
        }
        Command::PrepareArm9Control { source, out } => {
            let (b, _) = load(&source, &profile(None))?;
            arm9::prepare_control(&b, &out)?
        }
        Command::InspectArm9 { source, ram, out } => {
            let (b, _) = load(&source, &profile(None))?;
            let ram = ram.map(fs::read).transpose()?;
            arm9::inspect(&b, ram.as_deref(), &out)?
        }
        Command::InspectGuideObj { source, out } => {
            let (b, _) = load(&source, &profile(None))?;
            guide_obj::inspect(&Rom::parse(&b)?, &out)?
        }
        Command::PrepareGuideObj {
            source,
            translation,
            font,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            guide_obj::prepare(&Rom::parse(&b)?, &translation, &font, &out)?
        }
        Command::CheckGuideObjRam { rom, ram, out } => {
            let b = fs::read(rom)?;
            let r = guide_obj::residency(&Rom::parse(&b)?, &fs::read(ram)?)?;
            new_json(&out, &r)?;
            r
        }
        Command::InspectPauseTitle { ram, section, out } => {
            let r = pause::inspect_title(&fs::read(ram)?, &fs::read(section)?)?;
            new_json(&out, &r)?;
            r
        }
        Command::CheckPauseRam { rom, ram, out } => {
            let b = fs::read(rom)?;
            let report = pause::residency(&Rom::parse(&b)?, &fs::read(ram)?)?;
            new_json(&out, &report)?;
            report
        }
        Command::PreparePause {
            source,
            translation,
            font,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            pause::prepare(&Rom::parse(&b)?, &translation, &font, &out)?
        }
        Command::InspectBattleUi { source, out } => {
            let (b, _) = load(&source, &profile(None))?;
            battle_ui::inspect(&Rom::parse(&b)?, &out)?
        }
        Command::PrepareBattleUi {
            source,
            translation,
            font,
            button_font,
            small_font,
            name_font,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            battle_ui::prepare(
                &Rom::parse(&b)?,
                &translation,
                &font,
                &button_font,
                &small_font,
                name_font.as_deref(),
                &out,
            )?
        }
        Command::CheckBattleUiRam { rom, ram, out } => {
            let b = fs::read(rom)?;
            let report = battle_ui::residency(&Rom::parse(&b)?, &fs::read(ram)?)?;
            new_json(&out, &report)?;
            report
        }
        Command::ExtractArchive { source, file, out } => {
            let (b, _) = load(&source, &profile(None))?;
            assets::extract_archive(&Rom::parse(&b)?, &file, &out)?
        }
        Command::PrepareSelection {
            source,
            translation,
            font,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            selection::prepare(&Rom::parse(&b)?, &translation, &font, &out)?
        }
        Command::InspectSelection { source, out } => {
            let (b, _) = load(&source, &profile(None))?;
            selection::inspect(&Rom::parse(&b)?, &out)?
        }
        Command::InspectGuides { source, out } => {
            let (b, _) = load(&source, &profile(None))?;
            guides::inspect(&Rom::parse(&b)?, &out)?
        }
        Command::PrepareGuides {
            source,
            translation,
            font,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            guides::prepare(&Rom::parse(&b)?, &translation, &font, &out)?
        }
        Command::PrepareRulePanels {
            source,
            translation,
            font,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            panels::prepare(&Rom::parse(&b)?, &translation, &font, &out)?
        }
        Command::InspectRulePanels { source, out } => {
            let (b, _) = load(&source, &profile(None))?;
            panels::inspect(&Rom::parse(&b)?, &out)?
        }
        Command::InspectRuleScreens { source, out } => {
            let (b, _) = load(&source, &profile(None))?;
            screens::inspect(&Rom::parse(&b)?, &out)?
        }
        Command::PrepareRuleScreens {
            source,
            translation,
            title_artwork,
            title_font,
            font,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            screens::prepare(
                &Rom::parse(&b)?,
                &translation,
                title_artwork.as_deref(),
                title_font.as_deref(),
                &font,
                &out,
            )?
        }
        Command::CheckAcademyArtRam {
            rom,
            surface,
            ram,
            out,
        } => {
            let b = fs::read(rom)?;
            let r = crate::academy::logos::check_ram(&Rom::parse(&b)?, &surface, &fs::read(ram)?)?;
            new_json(&out, &r)?;
            r
        }
        Command::CheckWifiDialogSubstitution {
            rom,
            before,
            after,
            object,
            reference,
            number,
            out,
        } => {
            let b = fs::read(rom)?;
            let r = crate::wifi_dialog::check_substitution(
                &Rom::parse(&b)?,
                &fs::read(before)?,
                &fs::read(after)?,
                object,
                reference,
                &number,
            )?;
            new_json(&out, &r)?;
            r
        }
        Command::InspectWifiDialog { source, out } => {
            let (b, _) = load(&source, &profile(None))?;
            crate::wifi_dialog::inspect(&Rom::parse(&b)?, &out)?
        }
        Command::InspectTextFontObjects {
            rom,
            path,
            ram,
            out,
        } => {
            let b = fs::read(rom)?;
            let r = crate::text_objects::inspect(&Rom::parse(&b)?, &path, &fs::read(ram)?)?;
            new_json(&out, &r)?;
            r
        }
        Command::CheckWifiArtRam {
            rom,
            input_ram,
            list_ram,
            out,
        } => {
            let b = fs::read(rom)?;
            let r = crate::wifi_art::check_ram(
                &Rom::parse(&b)?,
                &fs::read(input_ram)?,
                &fs::read(list_ram)?,
            )?;
            new_json(&out, &r)?;
            r
        }
        Command::CheckTextureRam {
            rom,
            archive,
            table,
            ram,
            allow_copies,
            out,
        } => {
            let b = fs::read(rom)?;
            let r = graphics::check_texture_ram(
                &Rom::parse(&b)?,
                &archive,
                &table,
                &fs::read(ram)?,
                allow_copies,
            )?;
            new_json(&out, &r)?;
            r
        }
        Command::PrepareNames {
            source,
            translation,
            font,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            nameplates::prepare(&Rom::parse(&b)?, &translation, &font, &out)?
        }
        Command::TextCensus { source, out } => {
            let (b, _) = load(&source, &profile(None))?;
            corpus::census(&Rom::parse(&b)?, &out)?
        }
        Command::InspectTextControls { ram, out } => {
            let r = story::inspect_controls(&fs::read(ram)?)?;
            new_json(&out, &r)?;
            r
        }
        Command::PrepareStory {
            source,
            translation,
            font,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            story::prepare(&Rom::parse(&b)?, &translation, &font, &out)?
        }
        Command::ExportStory { source, path, out } => {
            let (b, _) = load(&source, &profile(None))?;
            story::export(&Rom::parse(&b)?, &path, &out)?
        }
        Command::MergeArchives {
            source,
            plan,
            out,
            supersede,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            archive::merge_plans(&Rom::parse(&b)?, &plan, &out, supersede.as_deref())?
        }
        Command::PrepareTitles {
            source,
            translation,
            font,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            titles::prepare(&Rom::parse(&b)?, &translation, &font, &out)?
        }
        Command::InspectTitles { source, out } => {
            let (b, _) = load(&source, &profile(None))?;
            titles::inspect(&Rom::parse(&b)?, &out)?
        }
        Command::PrepareAppreciation {
            source,
            translation,
            font,
            small_font,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            atlas::prepare_appreciation(&Rom::parse(&b)?, &translation, &font, &small_font, &out)?
        }
        Command::PrepareRules {
            source,
            translation,
            font,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            atlas::prepare_rules(&Rom::parse(&b)?, &translation, &font, &out)?
        }
        Command::PrepareSettings {
            source,
            translation,
            font,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            atlas::prepare_settings(&Rom::parse(&b)?, &translation, &font, &out)?
        }
        Command::InspectAcademySymbols { source, ram, out } => {
            let (b, _) = load(&source, &profile(None))?;
            crate::academy::symbols::inspect(&Rom::parse(&b)?, &fs::read(ram)?, &out)?
        }
        Command::InspectAcademyScripts { source, out } => {
            let (b, _) = load(&source, &profile(None))?;
            crate::academy::scripts::inspect(&Rom::parse(&b)?, &out)?
        }
        Command::PrepareAcademyList {
            source,
            surface,
            translation,
            font,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            crate::academy::lists::prepare(&Rom::parse(&b)?, &surface, &translation, &font, &out)?
        }
        Command::PrepareAcademyBattleLabels {
            source,
            translation,
            font,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            crate::academy::battle_labels::prepare(&Rom::parse(&b)?, &translation, &font, &out)?
        }
        Command::PrepareSprites {
            source,
            translation,
            font,
            small_font,
            medium_font,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            crate::sprites::prepare(
                &Rom::parse(&b)?,
                &translation,
                &font,
                &small_font,
                medium_font.as_deref(),
                &out,
            )?
        }
        Command::PrepareScreenText {
            source,
            translation,
            font,
            small_font,
            medium_font,
            narrow_font,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            crate::screen_text::prepare(
                &Rom::parse(&b)?,
                &translation,
                &font,
                &small_font,
                medium_font.as_deref(),
                narrow_font.as_deref(),
                &out,
            )?
        }
        Command::PrepareUnlock {
            source,
            translation,
            font,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            crate::unlock::prepare(&Rom::parse(&b)?, &translation, &font, &out)?
        }
        Command::PrepareLinearLabels {
            source,
            translation,
            font,
            small_font,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            crate::labels::prepare_linear(&Rom::parse(&b)?, &translation, &font, &small_font, &out)?
        }
        Command::UnlockStorySave { input, out } => crate::save::unlock_story(&input, &out)?,
        Command::PrepareLabels {
            source,
            translation,
            font,
            small_font,
            medium_font,
            narrow_font,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            crate::labels::prepare(
                &Rom::parse(&b)?,
                &translation,
                &font,
                &small_font,
                medium_font.as_deref(),
                narrow_font.as_deref(),
                &out,
            )?
        }
        Command::CopyDialogButtons {
            source,
            prepared,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            crate::wifi_dialog_art::copy(&Rom::parse(&b)?, &prepared, &out)?
        }
        Command::PrepareWifiDialogArt {
            source,
            translation,
            font,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            crate::wifi_dialog_art::prepare(&Rom::parse(&b)?, &translation, &font, &out)?
        }
        Command::PrepareWifiArt {
            source,
            translation,
            font,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            crate::wifi_art::prepare(&Rom::parse(&b)?, &translation, &font, &out)?
        }
        Command::PrepareSystemDialogs {
            source,
            translation,
            font,
            body_font,
            small_font,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            crate::system_dialogs::prepare(
                &Rom::parse(&b)?,
                &translation,
                &font,
                &small_font,
                body_font.as_deref(),
                &out,
            )?
        }
        Command::PrepareBattleStart {
            source,
            translation,
            font,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            crate::puyo_hud::start::prepare(&Rom::parse(&b)?, &translation, &font, &out)?
        }
        Command::PrepareNazoReach {
            source,
            translation,
            font,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            crate::puyo_hud::reach::prepare(&Rom::parse(&b)?, &translation, &font, &out)?
        }
        Command::PrepareBattleResults {
            source,
            translation,
            font,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            crate::puyo_hud::results::prepare(&Rom::parse(&b)?, &translation, &font, &out)?
        }
        Command::PrepareNazoPanels {
            source,
            translation,
            font,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            crate::puyo_hud::panels::prepare(&Rom::parse(&b)?, &translation, &font, &out)?
        }
        Command::PrepareNazoConditions {
            source,
            translation,
            font,
            narrow_font,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            crate::puyo_hud::conditions::prepare(
                &Rom::parse(&b)?,
                &translation,
                &font,
                narrow_font.as_deref(),
                &out,
            )?
        }
        Command::PrepareNazoCounter {
            source,
            translation,
            font,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            crate::puyo_hud::counter::prepare(&Rom::parse(&b)?, &translation, &font, &out)?
        }
        Command::PreparePuyoHudLabels {
            source,
            translation,
            font,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            crate::puyo_hud::labels::prepare(&Rom::parse(&b)?, &translation, &font, &out)?
        }
        Command::InspectPuyoHud { source, ram, out } => {
            let (b, _) = load(&source, &profile(None))?;
            crate::puyo_hud::inspect(&Rom::parse(&b)?, &fs::read(ram)?, &out)?
        }
        Command::InspectAcademyBattle { source, out } => {
            let (b, _) = load(&source, &profile(None))?;
            crate::academy::battle::inspect(&Rom::parse(&b)?, &out)?
        }
        Command::InspectAcademyLogos {
            source,
            challenge,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            crate::academy::logos::inspect(&Rom::parse(&b)?, challenge, &out)?
        }
        Command::PrepareAcademyLogos {
            source,
            challenge,
            translation,
            font,
            small_font,
            medium_font,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            crate::academy::logos::prepare(
                &Rom::parse(&b)?,
                challenge,
                &translation,
                &font,
                small_font.as_deref(),
                medium_font.as_deref(),
                &out,
            )?
        }
        Command::PrepareAcademyCaptions {
            source,
            translation,
            font,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            crate::academy::captions::prepare(&Rom::parse(&b)?, &translation, &font, &out)?
        }
        Command::PrepareAcademyButtons {
            source,
            translation,
            font,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            crate::academy::prepare_buttons(&Rom::parse(&b)?, &translation, &font, &out)?
        }
        Command::PrepareTokoLogos {
            source,
            translation,
            font,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            crate::toko::prepare_logos(&Rom::parse(&b)?, &translation, &font, &out)?
        }
        Command::PrepareTokoStatistics {
            source,
            translation,
            font,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            crate::toko::prepare_statistics(&Rom::parse(&b)?, &translation, &font, &out)?
        }
        Command::PrepareParticipationPanels {
            source,
            translation,
            font,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            crate::multiplayer::prepare_participation(&Rom::parse(&b)?, &translation, &font, &out)?
        }
        Command::PrepareMultiplayerPanels {
            source,
            translation,
            font,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            crate::multiplayer::prepare_panels(&Rom::parse(&b)?, &translation, &font, &out)?
        }
        Command::PrepareMultiplayerStatus {
            source,
            translation,
            font,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            crate::multiplayer::prepare_status(&Rom::parse(&b)?, &translation, &font, &out)?
        }
        Command::PreparePointLabels {
            source,
            translation,
            font,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            crate::point_get::prepare_labels(&Rom::parse(&b)?, &translation, &font, &out)?
        }
        Command::PreparePointTitle {
            source,
            translation,
            artwork,
            font,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            crate::point_get::prepare(
                &Rom::parse(&b)?,
                &translation,
                artwork.as_deref(),
                &font,
                &out,
            )?
        }
        Command::PrepareArtSheets { spec, out } => crate::art_pixels::prepare_sheets(&spec, &out)?,
        Command::PrepareArt { source, spec, out } => {
            let (b, _) = load(&source, &profile(None))?;
            crate::art_import::prepare(&Rom::parse(&b)?, &spec, &out)?
        }
        Command::CompareFontLibrary {
            source,
            inventory,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            crate::font_compare::run(&Rom::parse(&b)?, &inventory, &out)?
        }
        Command::InspectArtFonts {
            font,
            text_cells,
            out,
        } => {
            if text_cells {
                crate::art_pixels::text_font_gallery(&font, &out)?
            } else {
                crate::art_pixels::font_gallery(&font, &out)?
            }
        }
        Command::CompareArt {
            source,
            product,
            build,
            catalog,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            let k = fs::read(product)?;
            crate::art_review::compare::run(
                &Rom::parse(&b)?,
                &Rom::parse(&k)?,
                &catalog,
                &build,
                &out,
            )?
        }
        Command::CompareProducts {
            previous,
            previous_build,
            product,
            build,
            out,
        } => crate::art_review::product_diff::run(
            &previous,
            &previous_build,
            &product,
            &build,
            &out,
        )?,
        Command::InspectArtSource {
            source,
            catalog,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            crate::art_review::inspect_source(&Rom::parse(&b)?, &catalog, &out)?
        }
        Command::InspectArtCandidates {
            source,
            reference,
            product,
            build,
            catalog,
            out,
        } => {
            let (j, _) = load(&source, &profile(None))?;
            let (e, _) = load(&reference, &root().join("config/english-reference.json"))?;
            let k = fs::read(product)?;
            crate::art_review::inspect(
                [&Rom::parse(&j)?, &Rom::parse(&e)?, &Rom::parse(&k)?],
                &catalog,
                &build,
                &out,
            )?
        }
        Command::InspectTextures {
            source,
            archive,
            table,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            graphics::inspect_textures(&Rom::parse(&b)?, &archive, &table, &out)?
        }
        Command::InspectMenuGraphics { source, out } => {
            let (b, _) = load(&source, &profile(None))?;
            graphics::inspect_menu(&Rom::parse(&b)?, &out)?
        }
        Command::PrepareButtons {
            source,
            translation,
            font,
            out,
        } => {
            let (b, _) = load(&source, &profile(None))?;
            buttons::prepare(&Rom::parse(&b)?, &translation, &font, &out)?
        }
        Command::Identify { source } => {
            let b = fs::read(source)?;
            json!({"size_bytes":b.len(),"sha256":sha(&b)})
        }
        Command::VerifySource { source, profile: p } => {
            let (_, p) = load(&source, &profile(p))?;
            json!({"size_bytes":p.size_bytes,"sha256":p.sha256,"profile":p.id,"verification":"byte_identity_only"})
        }
        Command::Survey {
            source,
            profile: p,
            out,
        } => {
            let (b, p) = load(&source, &profile(p))?;
            let r = Rom::parse(&b)?;
            let v = assets::inventory(&r, &p)?;
            new_json(&out, &v)?;
            json!({"files":r.files.len(),"archives":v["archives"].as_array().unwrap().len(),"text_pairs":v["text_pairs"].as_array().unwrap().len(),"out":out})
        }
        Command::CompareScope {
            source,
            reference,
            product,
            build,
            translations,
            out,
        } => {
            let (j, _) = load(&source, &profile(None))?;
            let (e, _) = load(&reference, &root().join("config/english-reference.json"))?;
            let k = fs::read(product)?;
            assets::scope::compare(
                &Rom::parse(&j)?,
                &Rom::parse(&e)?,
                &Rom::parse(&k)?,
                &build,
                &translations,
                &out,
            )?
        }
        Command::CompareAssets {
            source,
            reference,
            out,
        } => {
            let (j, jp) = load(&source, &profile(None))?;
            let (e, ep) = load(&reference, &root().join("config/english-reference.json"))?;
            assets::compare_assets(&Rom::parse(&j)?, &Rom::parse(&e)?, &jp, &ep, &out)?
        }
        Command::ExtractFile {
            source,
            file,
            member,
            decoded,
            out,
        } => {
            ensure!(!out.exists(), "output exists");
            let (b, _) = load(&source, &profile(None))?;
            let r = Rom::parse(&b)?;
            let entry = r.file(&file)?;
            let archive;
            let raw = if let Some(id) = member {
                archive = unpack(r.data(entry))?;
                let n = Narc::parse(&archive)?;
                *n.members
                    .get(id)
                    .ok_or_else(|| anyhow::anyhow!("NARC member outside table"))?
            } else {
                r.data(entry)
            };
            let data = if decoded { unpack(raw)? } else { raw.to_vec() };
            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&out, &data)?;
            json!({"file_id":entry.id,"member_id":member,"size_bytes":data.len(),"sha256":sha(&data),"out":out})
        }
        Command::CheckMenuRam { source, ram, out } => {
            let (b, _) = load(&source, &profile(None))?;
            let v = menu::residency(&Rom::parse(&b)?, &fs::read(ram)?)?;
            new_json(&out, &v)?;
            v
        }
        Command::PrepareMenuPoc { source, ram, out } => {
            let (b, _) = load(&source, &profile(None))?;
            menu::prepare(
                &Rom::parse(&b)?,
                &fs::read(ram)?,
                &root().join("assets/fonts/poc"),
                &out,
            )?
        }
        Command::PrepareText {
            source,
            translation,
            font,
            out,
        } => prepare_text::run(source, translation, font, out)?,
        Command::CheckAcademyTextRam { rom, ram, out } => {
            let b = fs::read(rom)?;
            let r = crate::academy::symbols::check_ram(&Rom::parse(&b)?, &fs::read(ram)?)?;
            new_json(&out, &r)?;
            r
        }
        Command::CheckTextSourceRam {
            rom,
            path,
            ram,
            out,
        } => {
            let b = fs::read(rom)?;
            let r = localize::check_source_ram(&Rom::parse(&b)?, &path, &fs::read(ram)?)?;
            new_json(&out, &r)?;
            r
        }
        Command::CheckTextRam {
            rom,
            prepared,
            ram,
            out,
        } => {
            let b = fs::read(rom)?;
            let r = localize::check_ram(&Rom::parse(&b)?, &prepared, &fs::read(ram)?)?;
            new_json(&out, &r)?;
            r
        }
        Command::Build { source, plan, out } => build_rom::run(source, plan, out)?,
        Command::BuildProduct { source, spec, out } => {
            let (b, _) = load(&source, &profile(None))?;
            crate::product::run(&source, &b, &spec, &out)?
        }
    })
}
