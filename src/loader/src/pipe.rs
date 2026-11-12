use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::PathBuf,
};

use eframe::egui;

pub struct Pipe {
    path: PathBuf,
    file: File,
    pub next: u64,
}

impl Pipe {
    pub fn open(path: PathBuf, next: u64) -> Result<Self, String> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| format!("Cannot pipe to {}: {e}", path.display()))?;
        Ok(Self { path, file, next })
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    fn append(&mut self, lines: impl IntoIterator<Item = impl AsRef<str>>) -> std::io::Result<()> {
        let mut text = String::new();
        for line in lines {
            text.push_str(line.as_ref());
            text.push('\n');
        }
        if text.is_empty() {
            return Ok(());
        }
        self.file.write_all(text.as_bytes())
    }
}

pub trait Rows {
    fn end(&self) -> u64;
    fn shown_text(&self) -> String;
    fn pipe(&mut self) -> &mut Option<Pipe>;
    fn write_pipe(&mut self) -> Result<(), String>;
}

pub fn write(
    pipe: &mut Option<Pipe>,
    lines: impl IntoIterator<Item = impl AsRef<str>>,
) -> Result<(), String> {
    let Some(open) = pipe else {
        return Ok(());
    };
    let result = open
        .append(lines)
        .map_err(|e| format!("Stopped piping to {}: {e}", open.path.display()));
    if result.is_err() {
        *pipe = None;
    }
    result
}

pub fn toggle(
    pipe: &mut Option<Pipe>,
    file_name: &str,
    next: u64,
) -> Result<Option<String>, String> {
    if let Some(open) = pipe.take() {
        return Ok(Some(format!("Stopped piping to {}.", open.path.display())));
    }
    let Some(path) = rfd::FileDialog::new()
        .set_title("Pipe to a file (new lines are appended)")
        .add_filter("Text", &["txt"])
        .set_file_name(file_name)
        .save_file()
    else {
        return Ok(None);
    };
    let open = Pipe::open(path, next)?;
    let message = format!("Piping new lines to {}.", open.path.display());
    *pipe = Some(open);
    Ok(Some(message))
}

pub fn button(ui: &mut egui::Ui, pipe: Option<&Pipe>) -> bool {
    let response = ui.add(egui::Button::new("Pipe").selected(pipe.is_some()));
    match pipe {
        Some(pipe) => response.on_hover_text(format!(
            "Appending every new line to {}. Click to stop.",
            pipe.path.display()
        )),
        None => response.on_hover_text(
            "Appends every new line to a file you pick, and creates the file if it doesn't exist.",
        ),
    }
    .clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("volcren-pipe-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        let _ = std::fs::remove_file(&path);
        path
    }

    #[test]
    fn creates_the_file_and_appends_lines_to_what_it_holds() {
        let path = temp_file("appends.txt");
        let mut pipe = Some(Pipe::open(path.clone(), 0).unwrap());
        write(&mut pipe, ["one", "two"]).unwrap();
        write(&mut pipe, Vec::<String>::new()).unwrap();
        drop(pipe);
        let mut again = Some(Pipe::open(path.clone(), 0).unwrap());
        write(&mut again, ["three\nfour"]).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "one\ntwo\nthree\nfour\n"
        );
    }

    #[test]
    fn writing_to_no_pipe_does_nothing() {
        let mut pipe = None;
        assert_eq!(write(&mut pipe, ["line"]), Ok(()));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn a_failed_write_stops_the_pipe() {
        let mut pipe = Some(Pipe::open(PathBuf::from("/dev/full"), 0).unwrap());
        assert!(
            write(&mut pipe, ["line"])
                .unwrap_err()
                .starts_with("Stopped piping to /dev/full")
        );
        assert!(pipe.is_none());
    }

    #[test]
    fn a_folder_cannot_be_piped_to() {
        let dir = temp_file("folder");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(Pipe::open(dir, 0).is_err());
    }
}
