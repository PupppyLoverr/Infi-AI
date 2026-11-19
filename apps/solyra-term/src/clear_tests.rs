//! Regression tests for `clear`.
//!
//! Reported: `clear` does nothing visible in the zeron and Dove terminals. The
//! usual causes are (1) `CSI 2J` only blanking the screen while scrollback
//! stays, because only `CSI 3J` erases history, and (2) the viewport staying
//! scrolled into history so old output is still on screen. These tests drive
//! zgx's real terminal session, both with the raw sequences and with the
//! real `clear` program on a real pty, so a regression is caught where it
//! happens.

use std::time::{Duration, Instant};

use zgx_terminal::alacritty_terminal::{
    grid::Dimensions,
    index::{Column, Line},
};
use zgx_terminal::{GridSize, Session, SessionOptions};

const COLS: u16 = 40;
const ROWS: u16 = 10;

fn replay() -> Session {
    Session::replay(&SessionOptions::default(), GridSize::new(COLS, ROWS), || {})
}

fn lines(n: usize) -> Vec<u8> {
    (1..=n)
        .map(|i| format!("line {i}\r\n"))
        .collect::<String>()
        .into_bytes()
}

fn screen_rows(session: &Session) -> Vec<String> {
    let term = session.term();
    let grid = term.grid();
    (0..grid.screen_lines() as i32)
        .map(|l| {
            (0..grid.columns())
                .map(|c| grid[Line(l)][Column(c)].c)
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect()
}

fn history(session: &Session) -> usize {
    session.term().grid().history_size()
}

fn offset(session: &Session) -> usize {
    session.term().grid().display_offset()
}

#[test]
fn the_screen_is_blank_after_home_and_erase_display() {
    let s = replay();
    s.feed(&lines(30));
    assert!(history(&s) > 0, "scrolled output should be in history");
    s.feed(b"\x1b[H\x1b[2J");
    assert!(
        screen_rows(&s).iter().all(|r| r.is_empty()),
        "{:?}",
        screen_rows(&s)
    );
}

#[test]
fn erase_scrollback_empties_history() {
    let s = replay();
    s.feed(&lines(30));
    assert!(history(&s) > 0);
    s.feed(b"\x1b[3J");
    assert_eq!(history(&s), 0, "CSI 3J must erase scrollback");
}

#[test]
fn full_clear_sequence_leaves_nothing_behind() {
    // What ncurses `clear` sends for xterm-256color.
    let s = replay();
    s.feed(&lines(30));
    s.feed(b"\x1b[H\x1b[2J\x1b[3J");
    assert!(screen_rows(&s).iter().all(|r| r.is_empty()));
    assert_eq!(history(&s), 0);
    assert_eq!(offset(&s), 0);
}

/// If `CSI 2J` is all a program sends (older terminfo, many shells' Ctrl-L),
/// the visible screen must still be blank and at the bottom.
#[test]
fn two_j_alone_blanks_the_visible_screen() {
    let s = replay();
    s.feed(&lines(30));
    s.feed(b"\x1b[H\x1b[2Jprompt$ ");
    let rows = screen_rows(&s);
    assert_eq!(rows[0], "prompt$", "{rows:?}");
    assert!(rows[1..].iter().all(|r| r.is_empty()), "{rows:?}");
}

#[test]
fn new_output_returns_a_scrolled_up_view_to_the_bottom() {
    let s = replay();
    s.feed(&lines(40));
    s.term()
        .scroll_display(zgx_terminal::alacritty_terminal::grid::Scroll::Delta(5));
    assert!(offset(&s) > 0, "scrolled into history");
    // A clear sequence arrives while the user is looking at history.
    s.feed(b"\x1b[H\x1b[2J\x1b[3J");
    assert_eq!(
        offset(&s),
        0,
        "the view must be at the live screen after a clear"
    );
}

/// The real thing: the `clear` program on a real pty.
#[test]
fn the_real_clear_program_clears_the_screen_and_history() {
    if std::process::Command::new("clear")
        .arg("-V")
        .output()
        .is_err()
    {
        eprintln!("skipped: no `clear` binary on this machine");
        return;
    }
    let opts = SessionOptions {
        command: Some("seq 1 60; clear; printf AFTERCLEAR; sleep 2".into()),
        ..Default::default()
    };
    let s = Session::spawn(&opts, GridSize::new(COLS, ROWS), || {}).expect("a pty");
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let rows = screen_rows(&s);
        if rows.iter().any(|r| r.contains("AFTERCLEAR")) {
            let others: Vec<_> = rows
                .iter()
                .filter(|r| !r.is_empty() && !r.contains("AFTERCLEAR"))
                .collect();
            assert!(
                others.is_empty(),
                "old output visible after clear: {rows:?}"
            );
            assert_eq!(history(&s), 0, "scrollback survived `clear`");
            assert_eq!(offset(&s), 0);
            return;
        }
        assert!(Instant::now() < deadline, "no output after clear: {rows:?}");
        std::thread::sleep(Duration::from_millis(50));
    }
}
