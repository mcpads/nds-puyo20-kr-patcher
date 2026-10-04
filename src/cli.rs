use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    about = "NDS Puyo 20th source verification, asset analysis and bounded development builds"
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub command: Command,
}
#[derive(Subcommand)]
pub(crate) enum Command {
    CheckHangulNameBitmaps {
        rom: PathBuf,
        #[arg(long)]
        ram: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    CheckNameFontLoaded {
        rom: PathBuf,
        #[arg(long)]
        ram: PathBuf,
        #[arg(long)]
        object: usize,
        #[arg(long)]
        out: PathBuf,
    },
    /// Prepare an experimental expanded name font; runtime acceptance is separate.
    PrepareNameFont {
        source: PathBuf,
        #[arg(long)]
        font: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Measure a full modern Hangul candidate for the 8x8 name font.
    InspectNameHangul {
        source: PathBuf,
        #[arg(long)]
        font: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Inspect the separate Unicode-indexed 8x8 font and its ARM9 reader.
    InspectLcFont {
        source: PathBuf,
        #[arg(long)]
        ram: Option<PathBuf>,
        #[arg(long)]
        out: PathBuf,
    },
    CheckRecordsVram {
        rom: PathBuf,
        #[arg(long)]
        observation: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    PrepareRecordLabels {
        source: PathBuf,
        #[arg(long)]
        translation: PathBuf,
        #[arg(long)]
        font: PathBuf,
        #[arg(long)]
        small_font: PathBuf,
        #[arg(long)]
        narrow_font: Option<PathBuf>,
        #[arg(long)]
        medium_font: Option<PathBuf>,
        #[arg(long)]
        out: PathBuf,
    },
    PrepareRecordStatistics {
        source: PathBuf,
        #[arg(long)]
        translation: PathBuf,
        #[arg(long)]
        font: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    InspectRecordCaptions {
        source: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    InspectRecords {
        source: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    PrepareRecords {
        source: PathBuf,
        #[arg(long)]
        translation: PathBuf,
        #[arg(long)]
        font: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    CheckPointPanelRam {
        rom: PathBuf,
        #[arg(long)]
        ram: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    CheckPointTitleRam {
        rom: PathBuf,
        #[arg(long)]
        ram: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    CheckRecordsRam {
        rom: PathBuf,
        #[arg(long)]
        ram: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    CheckPauseTitleRam {
        rom: PathBuf,
        #[arg(long)]
        ram: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    PreparePauseTitle {
        source: PathBuf,
        #[arg(long)]
        translation: PathBuf,
        #[arg(long)]
        artwork: Option<PathBuf>,
        #[arg(long)]
        graphics_only: bool,
        #[arg(long)]
        font: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    CheckArm9Boot {
        rom: PathBuf,
        #[arg(long)]
        ram: PathBuf,
        #[arg(long)]
        observation: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    PrepareArm9Control {
        source: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    InspectArm9 {
        source: PathBuf,
        #[arg(long)]
        ram: Option<PathBuf>,
        #[arg(long)]
        out: PathBuf,
    },
    InspectGuideObj {
        source: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    PrepareGuideObj {
        source: PathBuf,
        #[arg(long)]
        translation: PathBuf,
        #[arg(long)]
        font: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    CheckGuideObjRam {
        rom: PathBuf,
        #[arg(long)]
        ram: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    InspectPauseTitle {
        #[arg(long)]
        ram: PathBuf,
        #[arg(long)]
        section: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    CheckPauseRam {
        rom: PathBuf,
        #[arg(long)]
        ram: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    PreparePause {
        source: PathBuf,
        #[arg(long)]
        translation: PathBuf,
        #[arg(long)]
        font: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    InspectBattleUi {
        source: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    PrepareBattleUi {
        source: PathBuf,
        #[arg(long)]
        translation: PathBuf,
        #[arg(long)]
        font: PathBuf,
        /// Galmuri9 for the large handicap buttons.
        #[arg(long)]
        button_font: PathBuf,
        /// Galmuri7 for the small handicap buttons.
        #[arg(long)]
        small_font: PathBuf,
        /// BM JUA for outlined battle names; absent keeps the Galmuri names.
        #[arg(long)]
        name_font: Option<PathBuf>,
        #[arg(long)]
        out: PathBuf,
    },
    CheckBattleUiRam {
        rom: PathBuf,
        #[arg(long)]
        ram: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    ExtractArchive {
        source: PathBuf,
        #[arg(long)]
        file: String,
        #[arg(long)]
        out: PathBuf,
    },
    PrepareSelection {
        source: PathBuf,
        #[arg(long)]
        translation: PathBuf,
        #[arg(long)]
        font: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    InspectSelection {
        source: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    InspectGuides {
        source: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    PrepareGuides {
        source: PathBuf,
        #[arg(long)]
        translation: PathBuf,
        #[arg(long)]
        font: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    PrepareRulePanels {
        source: PathBuf,
        #[arg(long)]
        translation: PathBuf,
        #[arg(long)]
        font: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    InspectRulePanels {
        source: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    InspectRuleScreens {
        source: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    PrepareRuleScreens {
        source: PathBuf,
        #[arg(long)]
        translation: PathBuf,
        #[arg(long)]
        title_artwork: Option<PathBuf>,
        #[arg(long)]
        title_font: Option<PathBuf>,
        #[arg(long)]
        font: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    CheckAcademyArtRam {
        rom: PathBuf,
        #[arg(long)]
        surface: String,
        #[arg(long)]
        ram: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    CheckWifiDialogSubstitution {
        rom: PathBuf,
        #[arg(long)]
        before: PathBuf,
        #[arg(long)]
        after: PathBuf,
        #[arg(long)]
        object: usize,
        #[arg(long)]
        reference: usize,
        #[arg(long)]
        number: String,
        #[arg(long)]
        out: PathBuf,
    },
    InspectWifiDialog {
        source: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    InspectTextFontObjects {
        rom: PathBuf,
        #[arg(long)]
        path: String,
        #[arg(long)]
        ram: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    CheckWifiArtRam {
        rom: PathBuf,
        #[arg(long)]
        input_ram: PathBuf,
        #[arg(long)]
        list_ram: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    CheckTextureRam {
        rom: PathBuf,
        #[arg(long)]
        archive: String,
        #[arg(long)]
        table: String,
        #[arg(long)]
        ram: PathBuf,
        /// Accept multiple complete copies, reporting every address without choosing one.
        #[arg(long)]
        allow_copies: bool,
        #[arg(long)]
        out: PathBuf,
    },
    PrepareNames {
        source: PathBuf,
        #[arg(long)]
        translation: PathBuf,
        #[arg(long)]
        font: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    TextCensus {
        source: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    InspectTextControls {
        #[arg(long)]
        ram: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    PrepareStory {
        source: PathBuf,
        #[arg(long)]
        translation: PathBuf,
        #[arg(long)]
        font: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    ExportStory {
        source: PathBuf,
        #[arg(long)]
        path: String,
        #[arg(long)]
        out: PathBuf,
    },
    MergeArchives {
        source: PathBuf,
        #[arg(long, required = true)]
        plan: Vec<PathBuf>,
        /// Exact previous/replacement member hashes permitted to supersede a writer.
        #[arg(long)]
        supersede: Option<PathBuf>,
        #[arg(long)]
        out: PathBuf,
    },
    PrepareTitles {
        source: PathBuf,
        #[arg(long)]
        translation: PathBuf,
        #[arg(long)]
        font: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    InspectTitles {
        source: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    PrepareAppreciation {
        source: PathBuf,
        #[arg(long)]
        translation: PathBuf,
        #[arg(long)]
        font: PathBuf,
        /// Galmuri9 for the three non-character section labels at 10px.
        #[arg(long)]
        small_font: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    PrepareRules {
        source: PathBuf,
        #[arg(long)]
        translation: PathBuf,
        #[arg(long)]
        font: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    PrepareSettings {
        source: PathBuf,
        #[arg(long)]
        translation: PathBuf,
        #[arg(long)]
        font: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    InspectAcademySymbols {
        source: PathBuf,
        #[arg(long)]
        ram: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    InspectAcademyScripts {
        source: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    PrepareAcademyList {
        source: PathBuf,
        #[arg(long, value_parser = ["guide", "practice", "challenge-menu", "challenge-play", "challenge-problem"])]
        surface: String,
        #[arg(long)]
        translation: PathBuf,
        #[arg(long)]
        font: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    PrepareAcademyBattleLabels {
        source: PathBuf,
        #[arg(long)]
        translation: PathBuf,
        #[arg(long)]
        font: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Redraw labels split across adjacent Gem/ILF sprites.
    PrepareSprites {
        source: PathBuf,
        #[arg(long)]
        translation: PathBuf,
        #[arg(long)]
        font: PathBuf,
        #[arg(long)]
        small_font: PathBuf,
        /// Third label face (`"font": "medium"`), e.g. Galmuri11 12px on narrow orbs.
        #[arg(long)]
        medium_font: Option<PathBuf>,
        #[arg(long)]
        out: PathBuf,
    },
    /// Draw text parts on 256x192 BG screens.
    PrepareScreenText {
        source: PathBuf,
        #[arg(long)]
        translation: PathBuf,
        #[arg(long)]
        font: PathBuf,
        #[arg(long)]
        small_font: PathBuf,
        #[arg(long)]
        medium_font: Option<PathBuf>,
        /// 12px face for `narrow_lines` (DenkiChip: 12px Hangul in a 10px advance).
        #[arg(long)]
        narrow_font: Option<PathBuf>,
        #[arg(long)]
        out: PathBuf,
    },
    /// Redraw the three text lines of unlock notice screens.
    PrepareUnlock {
        source: PathBuf,
        #[arg(long)]
        translation: PathBuf,
        #[arg(long)]
        font: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Redraw labels stored as linear 4bpp images.
    PrepareLinearLabels {
        source: PathBuf,
        #[arg(long)]
        translation: PathBuf,
        #[arg(long)]
        font: PathBuf,
        #[arg(long)]
        small_font: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Copy a DeSmuME save with every story character unlocked (observation aid).
    UnlockStorySave {
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Redraw texture-list text labels in declared rectangles.
    PrepareLabels {
        source: PathBuf,
        #[arg(long)]
        translation: PathBuf,
        #[arg(long)]
        font: PathBuf,
        #[arg(long)]
        small_font: PathBuf,
        /// Galmuri9 for labels with `"font": "medium"`.
        #[arg(long)]
        medium_font: Option<PathBuf>,
        /// DenkiChip for labels with `"font": "narrow"`.
        #[arg(long)]
        narrow_font: Option<PathBuf>,
        #[arg(long)]
        out: PathBuf,
    },
    /// Copy prepared connect dialog circles into archives reusing the same art.
    CopyDialogButtons {
        source: PathBuf,
        #[arg(long)]
        prepared: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    PrepareWifiDialogArt {
        source: PathBuf,
        #[arg(long)]
        translation: PathBuf,
        #[arg(long)]
        font: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    PrepareWifiArt {
        source: PathBuf,
        #[arg(long)]
        translation: PathBuf,
        #[arg(long)]
        font: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    PrepareSystemDialogs {
        source: PathBuf,
        #[arg(long)]
        translation: PathBuf,
        #[arg(long)]
        font: PathBuf,
        #[arg(long)]
        body_font: Option<PathBuf>,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value = "assets/fonts/galmuri7/Galmuri7.ttf")]
        small_font: PathBuf,
    },
    PrepareBattleStart {
        source: PathBuf,
        #[arg(long)]
        translation: PathBuf,
        #[arg(long)]
        font: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    PrepareNazoReach {
        source: PathBuf,
        #[arg(long)]
        translation: PathBuf,
        #[arg(long)]
        font: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    PrepareBattleResults {
        source: PathBuf,
        #[arg(long)]
        translation: PathBuf,
        #[arg(long)]
        font: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    PrepareNazoPanels {
        source: PathBuf,
        #[arg(long)]
        translation: PathBuf,
        #[arg(long)]
        font: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    PrepareNazoConditions {
        source: PathBuf,
        #[arg(long)]
        translation: PathBuf,
        #[arg(long)]
        font: PathBuf,
        /// Galmuri11 Condensed for fragments marked `narrow`.
        #[arg(long)]
        narrow_font: Option<PathBuf>,
        #[arg(long)]
        out: PathBuf,
    },
    PrepareNazoCounter {
        source: PathBuf,
        #[arg(long)]
        translation: PathBuf,
        #[arg(long)]
        font: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    PreparePuyoHudLabels {
        source: PathBuf,
        #[arg(long)]
        translation: PathBuf,
        #[arg(long)]
        font: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    InspectPuyoHud {
        source: PathBuf,
        #[arg(long)]
        ram: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    InspectAcademyBattle {
        source: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    InspectAcademyLogos {
        source: PathBuf,
        #[arg(long)]
        challenge: bool,
        #[arg(long)]
        out: PathBuf,
    },
    PrepareAcademyLogos {
        source: PathBuf,
        #[arg(long)]
        challenge: bool,
        #[arg(long)]
        translation: PathBuf,
        #[arg(long)]
        font: PathBuf,
        /// Galmuri11 for lettered titles marked `small`.
        #[arg(long)]
        small_font: Option<PathBuf>,
        /// Bold digit face (BM JUA) for lettered title numbers.
        #[arg(long)]
        medium_font: Option<PathBuf>,
        #[arg(long)]
        out: PathBuf,
    },
    PrepareAcademyCaptions {
        source: PathBuf,
        #[arg(long)]
        translation: PathBuf,
        #[arg(long)]
        font: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    PrepareAcademyButtons {
        source: PathBuf,
        #[arg(long)]
        translation: PathBuf,
        #[arg(long)]
        font: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    PrepareTokoLogos {
        source: PathBuf,
        #[arg(long)]
        translation: PathBuf,
        #[arg(long)]
        font: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    PrepareMultiplayerStatus {
        source: PathBuf,
        #[arg(long)]
        translation: PathBuf,
        #[arg(long)]
        font: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    PrepareParticipationPanels {
        source: PathBuf,
        #[arg(long)]
        translation: PathBuf,
        #[arg(long)]
        font: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    PrepareMultiplayerPanels {
        source: PathBuf,
        #[arg(long)]
        translation: PathBuf,
        #[arg(long)]
        font: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    PrepareTokoStatistics {
        source: PathBuf,
        #[arg(long)]
        translation: PathBuf,
        #[arg(long)]
        font: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    PreparePointLabels {
        source: PathBuf,
        #[arg(long)]
        translation: PathBuf,
        #[arg(long)]
        font: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    PreparePointTitle {
        source: PathBuf,
        #[arg(long)]
        translation: PathBuf,
        #[arg(long)]
        artwork: Option<PathBuf>,
        #[arg(long)]
        font: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Remove an explicitly declared solid production matte from authored PNG sheets.
    PrepareArtSheets {
        #[arg(long)]
        spec: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Prepare generated artwork for bounded ROM insertion.
    PrepareArt {
        source: PathBuf,
        #[arg(long)]
        spec: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    CompareFontLibrary {
        source: PathBuf,
        #[arg(long)]
        inventory: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    InspectArtFonts {
        #[arg(long, required = true)]
        font: Vec<PathBuf>,
        /// Compare the current story/menu cell renderer instead of decorative lettering.
        #[arg(long)]
        text_cells: bool,
        #[arg(long)]
        out: PathBuf,
    },
    /// Render catalog-selected original graphics without editing or recompressing.
    InspectArtSource {
        source: PathBuf,
        #[arg(long)]
        catalog: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Compare catalog graphics decoded directly from JP and the exact product ROM.
    CompareArt {
        source: PathBuf,
        #[arg(long)]
        product: PathBuf,
        #[arg(long)]
        build: PathBuf,
        #[arg(long)]
        catalog: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Compare FAT payloads and NARC members of two hash-bound product builds.
    CompareProducts {
        #[arg(long)]
        previous: PathBuf,
        #[arg(long)]
        previous_build: PathBuf,
        #[arg(long)]
        product: PathBuf,
        #[arg(long)]
        build: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    InspectArtCandidates {
        source: PathBuf,
        #[arg(long)]
        reference: PathBuf,
        #[arg(long)]
        product: PathBuf,
        #[arg(long)]
        build: PathBuf,
        #[arg(long)]
        catalog: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    InspectTextures {
        source: PathBuf,
        #[arg(long)]
        archive: String,
        #[arg(long)]
        table: String,
        #[arg(long)]
        out: PathBuf,
    },
    InspectMenuGraphics {
        source: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    PrepareButtons {
        source: PathBuf,
        #[arg(long)]
        translation: PathBuf,
        #[arg(long)]
        font: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    Identify {
        source: PathBuf,
    },
    VerifySource {
        source: PathBuf,
        #[arg(long)]
        profile: Option<PathBuf>,
    },
    Survey {
        source: PathBuf,
        #[arg(long)]
        profile: Option<PathBuf>,
        #[arg(long)]
        out: PathBuf,
    },
    CompareScope {
        source: PathBuf,
        #[arg(long)]
        reference: PathBuf,
        #[arg(long)]
        product: PathBuf,
        #[arg(long)]
        build: PathBuf,
        #[arg(long)]
        translations: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    CompareAssets {
        source: PathBuf,
        #[arg(long)]
        reference: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    ExtractFile {
        source: PathBuf,
        #[arg(long)]
        file: String,
        #[arg(long)]
        member: Option<usize>,
        #[arg(long)]
        decoded: bool,
        #[arg(long)]
        out: PathBuf,
    },
    CheckMenuRam {
        source: PathBuf,
        #[arg(long)]
        ram: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    PrepareMenuPoc {
        source: PathBuf,
        #[arg(long)]
        ram: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    PrepareText {
        source: PathBuf,
        #[arg(long, required = true)]
        translation: Vec<PathBuf>,
        #[arg(long)]
        font: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    CheckAcademyTextRam {
        rom: PathBuf,
        #[arg(long)]
        ram: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    CheckTextSourceRam {
        rom: PathBuf,
        #[arg(long)]
        path: String,
        #[arg(long)]
        ram: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    CheckTextRam {
        rom: PathBuf,
        #[arg(long)]
        prepared: PathBuf,
        #[arg(long)]
        ram: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    Build {
        source: PathBuf,
        #[arg(long, required = true)]
        plan: Vec<PathBuf>,
        #[arg(long)]
        out: PathBuf,
    },
    /// Build the complete development ROM from tracked sources in one step.
    BuildProduct {
        source: PathBuf,
        #[arg(long)]
        spec: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
}
