//! Single-file SFTP copy driven by [`SftpForm`]. Child module of `app` so it can reach `State`.

use iced::Task;

use super::{Message, Mode, State, ToastKind, expand_home};
use crate::sessions::{self, AppKnownHosts, ConnTarget};
use crate::sftp_form::{self, Direction, SftpForm};

fn size(n: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", UNITS[u])
    }
}

impl State {
    pub(super) fn open_sftp(&mut self) -> Task<Message> {
        let selected = self.list().selected_item().cloned();
        let host = selected.and_then(|i| match (i.group.is_none(), i.id) {
            (true, Some(id)) => Some((id, i.label)),
            _ => None,
        });
        let Some((id, label)) = host else {
            self.toast(ToastKind::Error, "Select a host first".to_owned());
            return Task::none();
        };
        self.sftp_form = Some(SftpForm::new(id, label));
        self.mode = Mode::Insert;
        self.matcher.reset();
        iced::widget::operation::focus(sftp_form::REMOTE_ID)
    }

    pub(super) fn edit_sftp(&mut self, f: impl FnOnce(&mut SftpForm)) -> Task<Message> {
        if let Some(form) = &mut self.sftp_form {
            f(form);
            form.error = None;
        }
        Task::none()
    }

    pub(super) fn sftp_done(&mut self, r: Result<String, String>) -> Task<Message> {
        match r {
            Ok(msg) => {
                self.sftp_form = None;
                self.mode = Mode::Normal;
                self.toast(ToastKind::Success, msg);
            }
            Err(e) => {
                if let Some(f) = &mut self.sftp_form {
                    f.busy = false;
                    f.error = Some(e);
                }
            }
        }
        Task::none()
    }

    pub(super) fn submit_sftp(&mut self) -> Task<Message> {
        let Some(form) = &mut self.sftp_form else {
            return Task::none();
        };
        if form.busy {
            return Task::none();
        }
        let direction = form.direction;
        let (remote, typed) = (form.remote.trim().to_owned(), form.local.trim().to_owned());
        if remote.is_empty() || typed.is_empty() {
            form.error = Some("enter both a remote and a local path".into());
            return Task::none();
        }
        form.busy = true;
        let host_id = form.host_id.clone();
        let (store, pool) = (self.store.clone(), self.pool.clone());
        Task::perform(
            async move {
                let params = sessions::resolve(&store, &ConnTarget::Saved(host_id.clone())).await?;
                let known = std::sync::Arc::new(AppKnownHosts(store));
                let conn = pool
                    .get_or_connect(&host_id, &params, known)
                    .await
                    .map_err(|e| e.to_string())?;
                let local = expand_home(&typed);
                match direction {
                    Direction::Download => {
                        let target = sftp_form::local_target(&local, &typed, &remote);
                        let n = tern_ssh::sftp::download(&conn, &remote, &target)
                            .await
                            .map_err(|e| e.to_string())?;
                        Ok(format!("Downloaded {} to {}", size(n), target.display()))
                    }
                    Direction::Upload => {
                        let target = sftp_form::remote_target(&remote, &local);
                        let n = tern_ssh::sftp::upload(&conn, &local, &target)
                            .await
                            .map_err(|e| e.to_string())?;
                        Ok(format!("Uploaded {} to {target}", size(n)))
                    }
                }
            },
            Message::SftpDone,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::size;

    #[test]
    fn sizes_are_human_readable() {
        assert_eq!(size(12), "12 B");
        assert_eq!(size(2048), "2.0 KiB");
        assert_eq!(size(5 * 1024 * 1024), "5.0 MiB");
    }
}
