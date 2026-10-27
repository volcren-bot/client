//! The webapp landing page's ASCII art (a flame over the VOLCREN lettering), painted faintly behind a page.
//! The colors follow `webapp/src/routes/+page.svelte`, without its shimmer.

use eframe::egui::{self, Color32, Rect, text::{LayoutJob, TextFormat}};

const FLAME: [&str; 34] = [
    "                      .=-",
    "                    .+%@",
    "                   =%#@@",
    "                 .*###@@",
    "                 *####@@",
    "                =#####@@",
    "                ######@@@",
    "               .%#####@@@@",
    "               .######@@@@@",
    "           .    *######@@@@@.",
    "          .*#+-.:%#######@@@@@",
    "           =###%+*########@@@@@",
    "           -###############@@@@@",
    "           *################@@@@@   ..",
    "          +#################@@@@@   :#+:",
    "        .*##################@@@@@@  .##%+.",
    "       -###################@@@@@@@   #####.",
    "     .+###################@@@@@@@   .%#####.",
    "    :###################@@@@@@@@    =######+",
    "   =%################@@@@@@@@@@    :%######%",
    "  +################@@@@@@@@@@    .+%#######%",
    " +##############@@@@@@@@@@@   .-*%#########+",
    "-##############@@@@@@@@@@  :=*%############.",
    "##############@@@@@@@@@  -*##############%-",
    "%#############@@@@@@@@  .-+@############%-",
    "%#############@@@@@@@      @@@##########:",
    "*#############@@@@@@@@.    @@@#######%=",
    ":##############@@@@@@@@@@@@@@@######*:",
    " -%#############@@@@@@@@@@@@######*:",
    "  :#################@@@@#######%+:",
    "    =#########################+.",
    "     .-*%#####################=.",
    "        .-+*%##################=",
    "            .:=+*##%####%%#*+-.",
];

#[derive(Clone, Copy)]
enum WordColor {
    Blue,
    Cyan,
    Purple,
}

const WORDMARK: [(WordColor, &str); 6] = [
    (WordColor::Blue, "██╗   ██╗ ██████╗ ██╗      ██████╗██████╗ ███████╗███╗   ██╗"),
    (WordColor::Blue, "██║   ██║██╔═══██╗██║     ██╔════╝██╔══██╗██╔════╝████╗  ██║"),
    (WordColor::Cyan, "██║   ██║██║   ██║██║     ██║     ██████╔╝█████╗  ██╔██╗ ██║"),
    (WordColor::Cyan, "╚██╗ ██╔╝██║   ██║██║     ██║     ██╔══██╗██╔══╝  ██║╚██╗██║"),
    (WordColor::Purple, " ╚████╔╝ ╚██████╔╝███████╗╚██████╗██║  ██║███████╗██║ ╚████║"),
    (WordColor::Purple, "  ╚═══╝   ╚═════╝ ╚══════╝ ╚═════╝╚═╝  ╚═╝╚══════╝╚═╝  ╚═══╝"),
];

const BLUE: Color32 = Color32::from_rgb(0x7a, 0xa2, 0xf7);
const CYAN: Color32 = Color32::from_rgb(0x7d, 0xcf, 0xff);
const PURPLE: Color32 = Color32::from_rgb(0xbb, 0x9a, 0xf7);
const YELLOW: Color32 = Color32::from_rgb(0xe0, 0xaf, 0x68);
const ORANGE: Color32 = Color32::from_rgb(0xff, 0x9e, 0x64);
const RED: Color32 = Color32::from_rgb(0xf7, 0x76, 0x8e);
const FG: Color32 = Color32::from_rgb(0xc0, 0xca, 0xf5);

/// How much of its color the art keeps over the page.
const STRENGTH: f32 = 0.14;
/// Width of a monospace glyph, in ems.
const GLYPH_WIDTH: f32 = 0.6;

/// Deterministic pseudo-random value in `[-1, 1)`.
fn jitter(i: usize, j: usize) -> f64 {
    let x = (i as f64 * 12.9898 + j as f64 * 78.233).sin() * 43758.5453;
    (x - x.floor()) * 2.0 - 1.0
}

/// The outer flame fades from yellow at the tip to red at the base, its bands bleeding into each other.
fn outer_color(i: usize, j: usize, width: usize, bias: f64) -> Color32 {
    let skew = (0.5 - j as f64 / width as f64) * 0.25;
    let t = (i as f64 + 0.5) / FLAME.len() as f64 + skew + bias * 0.07;
    [YELLOW, ORANGE, RED][((t * 3.0).floor().clamp(0.0, 2.0)) as usize]
}

/// The inner flame (`@`) stays bright, warming to light yellow along its left and bottom edges.
fn inner_color(i: usize, j: usize, bias: f64) -> Color32 {
    let row = FLAME[i].as_bytes();
    let mut d = 0;
    while j > d && row[j - d - 1] == b'@' {
        d += 1;
    }
    if FLAME.get(i + 1).and_then(|next| next.as_bytes().get(j)) != Some(&b'@') {
        d = 0;
    }
    let edge = (1.0 - d as f64 / 4.0).max(0.0);
    let amount = ((i as f64 + 0.5) / FLAME.len() as f64 - 0.25) * 1.6 * edge + bias * 0.3;
    let pct = (amount.clamp(0.0, 0.9) * 5.0).round() / 5.0;
    FG.lerp_to_gamma(YELLOW, pct as f32)
}

/// Font size of the art, in points; it only shrinks when the page is too narrow for it.
const FONT_SIZE: f32 = 9.0;

/// Paints the art centered in `rect` at a fixed size, under whatever is drawn next.
pub fn paint_background(ui: &egui::Ui, rect: Rect) {
    let columns = WORDMARK.iter().map(|(_, line)| line.chars().count()).max().unwrap_or(0);
    let size = FONT_SIZE.min(rect.width() * 0.92 / (columns as f32 * GLYPH_WIDTH));
    if size < 5.0 {
        return;
    }
    let width = FLAME.iter().map(|line| line.len()).max().unwrap_or(1);
    let format = |color: Color32| TextFormat {
        font_id: egui::FontId::monospace(size),
        line_height: Some(size),
        color: color.gamma_multiply(STRENGTH),
        ..Default::default()
    };
    let mut job = LayoutJob::default();
    // the flame is narrower than the lettering, so it is indented to sit over its middle
    let indent = " ".repeat(columns.saturating_sub(width) / 2);
    for (i, line) in FLAME.iter().enumerate() {
        job.append(&indent, 0.0, format(FG));
        // runs of one color, so a row is a few sections rather than one per character
        let mut run = String::new();
        let mut run_color = None;
        for (j, c) in line.char_indices() {
            let bias = jitter(i, j);
            let color = if c == '@' { inner_color(i, j, bias) } else { outer_color(i, j, width, bias) };
            if let Some(last) = run_color
                && last != color
            {
                job.append(&run, 0.0, format(last));
                run.clear();
            }
            run_color = Some(color);
            run.push(c);
        }
        if let Some(color) = run_color {
            job.append(&run, 0.0, format(color));
        }
        job.append("\n", 0.0, format(FG));
    }
    job.append("\n", 0.0, format(FG));
    for (color, line) in WORDMARK {
        let color = match color {
            WordColor::Blue => BLUE,
            WordColor::Cyan => CYAN,
            WordColor::Purple => PURPLE,
        };
        job.append(line, 0.0, format(color));
        job.append("\n", 0.0, format(color));
    }
    let painter = ui.painter_at(rect);
    let galley = painter.layout_job(job);
    painter.galley(rect.center() - galley.size() / 2.0, galley, FG);
}
