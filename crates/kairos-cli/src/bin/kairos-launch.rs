//! Windows 작업 스케줄러용 창 없는 실행기. 자세한 것은 `kairos_cli::launch`에 있다.
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    std::process::exit(kairos_cli::kairos_launch_main());
}
