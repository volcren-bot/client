use crate::theme::{BLUE, FG, PURPLE, RED, YELLOW};
use eframe::egui::{
    self, Color32, Rect,
    text::{LayoutJob, TextFormat},
};

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

const WORDMARK: [(Color32, &str); 6] = [
    (
        BLUE,
        "██╗   ██╗ ██████╗ ██╗      ██████╗██████╗ ███████╗███╗   ██╗",
    ),
    (
        BLUE,
        "██║   ██║██╔═══██╗██║     ██╔════╝██╔══██╗██╔════╝████╗  ██║",
    ),
    (
        CYAN,
        "██║   ██║██║   ██║██║     ██║     ██████╔╝█████╗  ██╔██╗ ██║",
    ),
    (
        CYAN,
        "╚██╗ ██╔╝██║   ██║██║     ██║     ██╔══██╗██╔══╝  ██║╚██╗██║",
    ),
    (
        PURPLE,
        " ╚████╔╝ ╚██████╔╝███████╗╚██████╗██║  ██║███████╗██║ ╚████║",
    ),
    (
        PURPLE,
        "  ╚═══╝   ╚═════╝ ╚══════╝ ╚═════╝╚═╝  ╚═╝╚══════╝╚═╝  ╚═══╝",
    ),
];

const CYAN: Color32 = Color32::from_rgb(0x7d, 0xcf, 0xff);
const ORANGE: Color32 = Color32::from_rgb(0xff, 0x9e, 0x64);

const STRENGTH: f32 = 0.14;
const GLYPH_WIDTH: f32 = 0.6;

fn jitter(i: usize, j: usize) -> f64 {
    let x = (i as f64 * 12.9898 + j as f64 * 78.233).sin() * 43758.5453;
    (x - x.floor()) * 2.0 - 1.0
}

fn outer_color(i: usize, j: usize, width: usize, bias: f64) -> Color32 {
    let skew = (0.5 - j as f64 / width as f64) * 0.25;
    let t = (i as f64 + 0.5) / FLAME.len() as f64 + skew + bias * 0.07;
    [YELLOW, ORANGE, RED][((t * 3.0).floor().clamp(0.0, 2.0)) as usize]
}

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

const FONT_SIZE: f32 = 9.0;

pub fn paint_background(ui: &egui::Ui, rect: Rect) {
    let columns = WORDMARK
        .iter()
        .map(|(_, line)| line.chars().count())
        .max()
        .unwrap_or(0);
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
    let indent = " ".repeat(columns.saturating_sub(width) / 2);
    for (i, line) in FLAME.iter().enumerate() {
        job.append(&indent, 0.0, format(FG));
        let mut run = String::new();
        let mut run_color = None;
        for (j, c) in line.char_indices() {
            let bias = jitter(i, j);
            let color = if c == '@' {
                inner_color(i, j, bias)
            } else {
                outer_color(i, j, width, bias)
            };
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
        job.append(line, 0.0, format(color));
        job.append("\n", 0.0, format(color));
    }
    let painter = ui.painter_at(rect);
    let galley = painter.layout_job(job);
    painter.galley(rect.center() - galley.size() / 2.0, galley, FG);
}
