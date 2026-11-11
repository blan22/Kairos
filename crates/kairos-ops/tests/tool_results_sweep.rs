//! 도구 결과 임시 파일 정리 시험.
#![allow(clippy::unwrap_used)]

mod common;
mod tool_results_support;

use std::fs;
use std::os::unix::fs::symlink;

use common::TestHome;
use kairos_ops::{ToolResults, ToolResultsSweep};
use tool_results_support::*;

const fn sweep(removed: usize, remaining: usize) -> ToolResultsSweep {
    ToolResultsSweep { removed, remaining }
}

#[test]
fn 정리는_tool_results_파일만_지우고_빈_폴더를_치운다() {
    let paths = Paths::new();
    fs::write(&paths.file, "a").unwrap();
    fs::write(paths.results_dir.join("toolu_second.txt"), "b").unwrap();
    assert_eq!(paths.results().sweep().unwrap(), sweep(2, 0));
    assert!(!paths.project.exists());
}

#[test]
fn 정리는_토큰_상한_초과분의_mcp_파일도_지우고_그_밖의_이름은_남긴다() {
    let paths = Paths::new();
    fs::write(&paths.file, "a").unwrap();
    fs::write(paths.results_dir.join("mcp-synth_server-get_thing-1730700000000.txt"), "b").unwrap();
    fs::write(paths.results_dir.join("notes.txt"), "c").unwrap();
    assert_eq!(paths.results().sweep().unwrap(), sweep(2, 1));
    assert!(paths.results_dir.join("notes.txt").exists());
}

#[test]
fn 정리_범위_다른_프로젝트_세션의_다른_파일_프로젝트_밖은_지우지_않는다() {
    let paths = Paths::new();
    fs::write(&paths.file, "a").unwrap();
    let sibling_name = format!("{}-sibling", paths.project.file_name().unwrap().to_string_lossy());
    let sibling = paths.project.parent().unwrap().join(sibling_name).join(SESSION).join("tool-results");
    fs::create_dir_all(&sibling).unwrap();
    fs::write(sibling.join(FILE), "keep").unwrap();
    let transcript = paths.project.join(SESSION).join("transcript.jsonl");
    fs::write(&transcript, "keep").unwrap();
    let memory = paths.project.join("memory.md");
    fs::write(&memory, "keep").unwrap();
    let other = paths.project.join(SESSION).join("subagents");
    fs::create_dir(&other).unwrap();
    fs::write(other.join("x.txt"), "keep").unwrap();
    let outside = paths.test.join("outside.txt");
    fs::write(&outside, "keep").unwrap();

    assert_eq!(paths.results().sweep().unwrap(), sweep(1, 0));
    assert!(!paths.results_dir.exists());
    for path in [sibling.join(FILE), transcript, memory, other.join("x.txt"), outside] {
        assert_eq!(fs::read_to_string(&path).unwrap(), "keep", "{}", path.display());
    }
}

#[test]
fn 정리는_링크를_따라가지_않고_링크만_지운다() {
    let paths = Paths::new();
    let elsewhere = paths.test.join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    fs::write(elsewhere.join("keep.txt"), "keep").unwrap();
    symlink(elsewhere.join("keep.txt"), paths.results_dir.join("toolu_file-link.txt")).unwrap();
    let linked_results = paths.project.join("00000000-0000-4000-8000-000000000003");
    fs::create_dir(&linked_results).unwrap();
    symlink(&elsewhere, linked_results.join("tool-results")).unwrap();
    let real_session = paths.test.join("real-session");
    fs::create_dir_all(real_session.join("tool-results")).unwrap();
    fs::write(real_session.join("tool-results").join("keep.txt"), "keep").unwrap();
    symlink(&real_session, paths.project.join("00000000-0000-4000-8000-000000000004")).unwrap();

    assert_eq!(paths.results().sweep().unwrap(), sweep(2, 0));
    let names: Vec<_> = fs::read_dir(&elsewhere).unwrap().map(|entry| entry.unwrap().file_name()).collect();
    assert_eq!(names, ["keep.txt"]);
    assert_eq!(fs::read_to_string(real_session.join("tool-results").join("keep.txt")).unwrap(), "keep");
    assert_eq!(fs::read_to_string(elsewhere.join("keep.txt")).unwrap(), "keep");
}

#[test]
fn 프로젝트_폴더_자체가_링크면_아무것도_하지_않는다() {
    let paths = Paths::new();
    fs::write(paths.results_dir.join("x.txt"), "keep").unwrap();
    let link = paths.test.join("link-project");
    symlink(&paths.project, &link).unwrap();
    assert_eq!(ToolResults::new(&link).sweep().unwrap(), sweep(0, 0));
    assert_eq!(fs::read_to_string(paths.results_dir.join("x.txt")).unwrap(), "keep");
}

#[test]
fn 폴더는_재귀로_지우지_않고_남은_건수로_드러낸다() {
    let paths = Paths::new();
    fs::create_dir(paths.results_dir.join("nested")).unwrap();
    fs::write(paths.results_dir.join("nested").join("x.txt"), "keep").unwrap();
    assert_eq!(paths.results().sweep().unwrap(), sweep(0, 1));
    assert_eq!(fs::read_to_string(paths.results_dir.join("nested").join("x.txt")).unwrap(), "keep");
}

#[test]
fn 폴더가_없으면_조용히_0건이다() {
    let test = TestHome::new();
    let none = ToolResults::new(test.join("none"));
    assert_eq!(none.sweep().unwrap(), sweep(0, 0));
    assert!(none.entries().unwrap().is_empty());
}

#[test]
fn 정리는_수집기_세션의_하네스_파일만_지우고_나머지는_남은_건수로_드러낸다() {
    let paths = Paths::new();
    fs::write(&paths.file, "a").unwrap();
    fs::write(paths.results_dir.join("notes.txt"), "keep").unwrap();
    let stranger = paths.project.join("not-uuid").join("tool-results");
    fs::create_dir_all(&stranger).unwrap();
    fs::write(stranger.join("toolu_x.txt"), "keep").unwrap();
    assert_eq!(paths.results().sweep().unwrap(), sweep(1, 2));
    assert_eq!(fs::read_to_string(paths.results_dir.join("notes.txt")).unwrap(), "keep");
    assert_eq!(fs::read_to_string(stranger.join("toolu_x.txt")).unwrap(), "keep");
}

#[test]
fn 세션_기록이_있는_폴더는_대화형_세션이라_지우지도_세지도_않는다() {
    let paths = Paths::new();
    fs::write(&paths.file, "keep").unwrap();
    fs::write(paths.project.join(format!("{SESSION}.jsonl")), "{}").unwrap();
    assert_eq!(paths.results().sweep().unwrap(), sweep(0, 0));
    assert_eq!(fs::read_to_string(&paths.file).unwrap(), "keep");
}

// 아래는 원본에 없는 시험이다.

#[test]
fn 항목은_tool_results_밖의_세션_파일을_세지_않고_링크_자체를_센다() {
    let paths = Paths::new();
    fs::write(&paths.file, "a").unwrap();
    fs::write(paths.project.join(SESSION).join("other.txt"), "x").unwrap();
    let linked = paths.project.join("00000000-0000-4000-8000-000000000005");
    fs::create_dir(&linked).unwrap();
    symlink(paths.test.path(), linked.join("tool-results")).unwrap();
    let mut entries = paths.results().entries().unwrap();
    entries.sort();
    let mut expected = vec![paths.file.clone(), linked.join("tool-results")];
    expected.sort();
    assert_eq!(entries, expected);
}

#[test]
fn tool_results라는_이름의_파일은_세션_링크로_착각하지_않는다() {
    let paths = Paths::new();
    fs::write(paths.results_dir.join("tool-results"), "keep").unwrap();
    assert_eq!(paths.results().sweep().unwrap(), sweep(0, 1));
    assert!(paths.results_dir.join("tool-results").exists());
}
