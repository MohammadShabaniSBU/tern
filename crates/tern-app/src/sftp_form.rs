//! Tiny dialog for copying one file to or from the selected host over SFTP.

use iced::Element;
use iced::widget::{column, row, text, text_input};

use crate::select::{self, SelectSpec};

use crate::app::Message;
use crate::theme::tokens::*;

pub const REMOTE_ID: &str = "sftp-remote";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Download,
    Upload,
}

impl Direction {
    pub const ALL: [Direction; 2] = [Direction::Download, Direction::Upload];

    pub fn label(self) -> &'static str {
        match self {
            Self::Download => "Download (host → this machine)",
            Self::Upload => "Upload (this machine → host)",
        }
    }
}

#[derive(Debug)]
pub struct SftpForm {
    pub host_id: String,
    pub host_label: String,
    pub remote: String,
    pub local: String,
    pub direction: Direction,
    pub error: Option<String>,
    pub busy: bool,
}

impl SftpForm {
    pub fn new(host_id: String, host_label: String) -> Self {
        Self {
            host_id,
            host_label,
            remote: String::new(),
            local: "~/".into(),
            direction: Direction::Download,
            error: None,
            busy: false,
        }
    }
}

/// The local file a download writes to: a directory (or trailing `/`) gets the remote file name.
pub fn local_target(local: &std::path::Path, typed: &str, remote: &str) -> std::path::PathBuf {
    if typed.ends_with('/') || local.is_dir() {
        let name = remote
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or("");
        local.join(name)
    } else {
        local.to_path_buf()
    }
}

/// The remote file an upload writes to: a trailing `/` gets the local file name.
pub fn remote_target(remote: &str, local: &std::path::Path) -> String {
    if remote.ends_with('/') {
        let name = local
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        format!("{remote}{name}")
    } else {
        remote.to_owned()
    }
}

fn field<'a>(
    placeholder: &str,
    value: &str,
    on_input: fn(String) -> Message,
) -> text_input::TextInput<'a, Message> {
    text_input(placeholder, value)
        .style(crate::theme::style::input(false))
        .on_input(on_input)
        .on_submit(Message::SftpSubmit)
        .padding(SPACE_4)
}

fn labeled<'a>(label: &'static str, input: Element<'a, Message>) -> Element<'a, Message> {
    row![text(label).size(TEXT_BODY).width(120), input]
        .spacing(SPACE_8)
        .align_y(iced::Center)
        .into()
}

pub fn view(form: &SftpForm) -> Element<'_, Message> {
    let mut body =
        column![text(format!("Copy a file to or from {}", form.host_label)).size(TEXT_TITLE)]
            .spacing(SPACE_8);
    body = body.push(labeled(
        "Remote path",
        field("/home/user/file.txt", &form.remote, Message::SftpRemote)
            .id(REMOTE_ID)
            .into(),
    ));
    body = body.push(labeled(
        "Local path",
        field("~/Downloads/", &form.local, Message::SftpLocal).into(),
    ));
    body = body.push(labeled(
        "Direction",
        select::view(SelectSpec {
            id: "sftp-direction",
            labels: Direction::ALL
                .iter()
                .map(|d| d.label().to_owned())
                .collect(),
            current: Direction::ALL
                .iter()
                .position(|d| *d == form.direction)
                .unwrap_or(0),
            picks: Direction::ALL
                .iter()
                .map(|d| Message::SftpDirection(*d))
                .collect(),
        }),
    ));
    if form.busy {
        body = body.push(text("Copying…").size(TEXT_BODY));
    }
    if let Some(e) = &form.error {
        body = body.push(
            text(e.as_str())
                .size(TEXT_BODY)
                .style(crate::theme::style::text_error),
        );
    }
    body = body.push(
        text("A trailing / on the destination keeps the file name. Downloads never overwrite.")
            .size(TEXT_SMALL),
    );
    body = body.push(text("Enter / Ctrl+S copy   Tab next field   Esc cancel").size(TEXT_SMALL));
    crate::theme::widgets::dialog(body, 640.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn download_into_a_folder_keeps_the_remote_name() {
        let p = local_target(Path::new("/tmp/dl/"), "/tmp/dl/", "/srv/a/report.txt");
        assert_eq!(p, Path::new("/tmp/dl/report.txt"));
        let p = local_target(Path::new("/tmp/x.txt"), "/tmp/x.txt", "/srv/a/report.txt");
        assert_eq!(p, Path::new("/tmp/x.txt"));
    }

    #[test]
    fn upload_into_a_remote_folder_keeps_the_local_name() {
        assert_eq!(
            remote_target("/srv/in/", Path::new("/home/me/a.txt")),
            "/srv/in/a.txt"
        );
        assert_eq!(
            remote_target("/srv/in/b", Path::new("/x/a.txt")),
            "/srv/in/b"
        );
    }
}
