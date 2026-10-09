// SPDX-License-Identifier: MIT

//! Does the screen model show what tmux itself shows? A real tmux client on a pseudo-terminal
//! (a private server, never the default one) draws a pane; its output is fed to a `ScreenModel`;
//! the model's rows must equal `tmux capture-pane` of the same pane. Skipped without tmux.

use flight_client::ScreenModel;
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::io::Read;
use std::process::Command;
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

const SCRIPT: &str = r#"
printf 'plain ascii line\n'
printf '\033[1mbold\033[0m \033[7mreverse\033[0m \033[4munderline\033[0m \033[3mitalic\033[0m\n'
for i in 0 1 2 3 4 5 6 7; do printf '\033[48;5;%dm  \033[0m' $((i*30+16)); done; printf '\n'
printf '\033[38;2;255;100;0mtruecolor\033[0m\n'
printf 'cjk: 日本語のテキスト and wide: 🙂🙂 done\n'
printf 'combining: e\xcc\x81 a\xcc\x88 done\n'
printf 'box: ┌──┬──┐ │  │  │ └──┴──┘\n'
printf 'tab:\tcol8\tcol16\n'
printf '%0120d\n' 7
printf 'after the wrapped line\n'
for i in $(seq 1 40); do printf 'scroll line %02d\n' $i; done
printf 'last visible line\n'
exec cat
"#;

/// The same lines without the scrolling, so every line is still on screen.
fn short_script() -> String {
    let (head, _) = SCRIPT.split_once("printf '%0120d\\n' 7").unwrap();
    format!("{head}exec cat\n")
}

struct Rig {
    socket: String,
    master: Box<dyn MasterPty + Send>,
    output: Receiver<Vec<u8>>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
}

fn tmux(socket: &str, args: &[&str]) -> String {
    let out = Command::new("tmux")
        .args(["-L", socket, "-f", "/dev/null"])
        .args(args)
        .output()
        .expect("tmux");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

impl Rig {
    fn start(tag: &str, cols: u16, rows: u16) -> Option<Self> {
        Self::start_with(tag, cols, rows, SCRIPT)
    }

    fn start_with(tag: &str, cols: u16, rows: u16, script_text: &str) -> Option<Self> {
        Command::new("tmux").arg("-V").output().ok()?;
        let socket = format!("flight-fidelity-{}-{tag}", std::process::id());
        let script =
            std::env::temp_dir().join(format!("flight-fidelity-{}-{tag}.sh", std::process::id()));
        std::fs::write(&script, script_text).ok()?;
        tmux(
            &socket,
            &[
                "new-session",
                "-d",
                "-s",
                "t",
                "-x",
                &cols.to_string(),
                "-y",
                &rows.to_string(),
                &format!("sh {}", script.display()),
            ],
        );
        std::thread::sleep(Duration::from_millis(600));
        let pair = native_pty_system()
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .ok()?;
        let mut cmd = CommandBuilder::new("tmux");
        cmd.args(["-L", &socket, "-f", "/dev/null", "attach", "-t", "t"]);
        cmd.env("TERM", "xterm-256color");
        let child = pair.slave.spawn_command(cmd).ok()?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().ok()?;
        let (tx, output) = channel();
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                    break;
                }
            }
        });
        Some(Self {
            socket,
            master: pair.master,
            output,
            child,
        })
    }

    /// Everything the client wrote until it has been quiet for a moment.
    fn drain(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut quiet_since = Instant::now();
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline && quiet_since.elapsed() < Duration::from_millis(400) {
            if let Ok(chunk) = self.output.recv_timeout(Duration::from_millis(50)) {
                out.extend(chunk);
                quiet_since = Instant::now();
            }
        }
        out
    }

    fn capture(&self) -> Vec<String> {
        tmux(&self.socket, &["capture-pane", "-p", "-t", "t"])
            .lines()
            .map(|l| l.trim_end().to_owned())
            .collect()
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = tmux(&self.socket, &["kill-server"]);
    }
}

fn pane_rows(model: &ScreenModel, rows: u16) -> Vec<String> {
    (0..rows.saturating_sub(1))
        .map(|r| model.row_text(r))
        .collect()
}

#[test]
fn the_model_shows_what_tmux_shows_for_text_wide_characters_and_scrolling() {
    let Some(rig) = Rig::start("text", 100, 30) else {
        return;
    };
    let mut model = ScreenModel::new(100, 30);
    assert!(model.feed(&rig.drain()));
    let want = rig.capture();
    let got = pane_rows(&model, 30);
    for (i, row) in got.iter().enumerate() {
        assert_eq!(row, want.get(i).map_or("", String::as_str), "row {i}");
    }
    // The status line is tmux's own, drawn by the client under the pane.
    assert!(
        model.row_text(29).contains("[t] 0:"),
        "{:?}",
        model.row_text(29)
    );
}

#[test]
fn attributes_and_colours_reach_the_cells() {
    let Some(rig) = Rig::start_with("attrs", 100, 30, &short_script()) else {
        return;
    };
    let mut model = ScreenModel::new(100, 30);
    model.feed(&rig.drain());
    // Find the rows by their text: the script scrolled, so they are where tmux put them.
    let row_of = |needle: &str| (0..29u16).find(|r| model.row_text(*r).contains(needle));
    let bold_row = row_of("bold").expect("the attribute line is on screen");
    let line = model.row_text(bold_row);
    let col = |word: &str| u16::try_from(line.find(word).unwrap()).unwrap();
    assert!(model.cell(col("bold"), bold_row).unwrap().bold);
    assert!(model.cell(col("reverse"), bold_row).unwrap().inverse);
    assert!(model.cell(col("underline"), bold_row).unwrap().underline);
    assert!(model.cell(col("italic"), bold_row).unwrap().italic);
    let tc = row_of("truecolor").expect("the truecolour line");
    let c = model.cell(0, tc).unwrap();
    // tmux may pass 24-bit colour through or reduce it to the 256-colour cube; either way it is
    // a colour, not the default.
    assert!(c.fg != flight_client::Colour::Default, "{c:?}");
}

#[test]
fn after_a_resize_the_surface_repaints_and_the_model_follows() {
    let Some(rig) = Rig::start("resize", 100, 30) else {
        return;
    };
    let mut model = ScreenModel::new(100, 30);
    model.feed(&rig.drain());
    // The terminal is made smaller: the client is told, the model is told, tmux repaints.
    rig.master
        .resize(PtySize {
            rows: 20,
            cols: 60,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    model.resize(60, 20);
    assert_eq!(model.size(), (60, 20));
    std::thread::sleep(Duration::from_millis(500));
    assert!(model.feed(&rig.drain()));
    let want = rig.capture();
    let got = pane_rows(&model, 20);
    for (i, row) in got.iter().enumerate() {
        assert_eq!(
            row,
            want.get(i).map_or("", String::as_str),
            "row {i} after the resize"
        );
    }
    assert!(model.row_text(19).contains("[t] 0:"));
}
