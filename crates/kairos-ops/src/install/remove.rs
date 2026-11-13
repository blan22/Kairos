//! 설치본과 데이터 폴더 지우기. `kairos uninstall --runtime`, `--purge`가 쓴다.
//!
//! `KAIROS_HOME`을 잘못 준 채로 남의 폴더를 지우지 않게 모양을 좁게 본다. 데이터 폴더가 링크이거나 홈 폴더나 그 조상이면
//! 실행본도 데이터도 지우지 않는다. 실행본은 데이터 폴더 바로 아래에서 이름이 판 번호이고 `bin/kairos`와 `bin/kairos-run`이
//! 든 진짜 폴더뿐이고, 명령 링크는 이 데이터 폴더를 가리키는 심볼릭 링크일 때만 지운다. 데이터 폴더 전체는 kairos만 쓰는
//! 이름의 파일(표지)이 있을 때만 지운다.

use std::fs;
use std::path::{Component, Path, PathBuf};

use kairos_core::KairosResult;

use crate::fs_util::remove_if_present;

/// 데이터 폴더의 표지. 하나라도 있어야 kairos 데이터 폴더로 본다. `config.toml`, `current`처럼 다른 도구도 쓰는 이름은 뺀다.
const HOME_MARKERS: [&str; 3] = ["kairos.db", "connections.toml", "install-source"];
/// 실행본의 `bin`에 함께 있어야 하는 파일.
const RUNTIME_BINARIES: [&str; 2] = ["kairos", "kairos-run"];
const CURRENT_LINK: &str = "current";

/// 지울 것의 목록. 계획만 세우고 지우지 않는다. dry-run과 확인 문구가 이 목록을 보여 준다.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RemovalPlan {
    /// 실행본 폴더(`<데이터 폴더>/<판>`), 이름순.
    pub runtimes: Vec<PathBuf>,
    /// `<데이터 폴더>/current` 링크.
    pub current: Option<PathBuf>,
    /// `~/.local/bin/kairos`. 이 데이터 폴더를 가리키는 링크일 때만.
    pub command_link: Option<PathBuf>,
}

impl RemovalPlan {
    pub fn is_empty(&self) -> bool {
        self.runtimes.is_empty() && self.current.is_none() && self.command_link.is_none()
    }

    /// 실행본 안의 알림 헬퍼 번들. LaunchServices에서 빼려고 지우기 전에 읽는다.
    pub fn notifier_apps(&self) -> Vec<PathBuf> {
        self.runtimes
            .iter()
            .map(|runtime| runtime.join("ops").join("notifier").join("KairosNotifier.app"))
            .filter(|app| app.is_dir())
            .collect()
    }

    /// 실행본 안의 Orca 패널 폴더. 내리는 명령을 알려 주려고 읽는다.
    pub fn panel_dirs(&self) -> Vec<PathBuf> {
        self.runtimes
            .iter()
            .map(|runtime| runtime.join("plugins").join("orca-panel"))
            .filter(|dir| dir.is_dir())
            .collect()
    }
}

/// 링크가 아닌 폴더인가.
fn is_real_dir(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir())
}

/// 끝의 `/`와 `.`를 뗀 경로. `<링크>/`는 링크를 따라가므로 검사하기 전에 뗀다.
fn settled(path: &Path) -> PathBuf {
    path.components().collect()
}

/// 판 번호 모양의 이름인가(`0.2.0`, `0.3.0-rc.1`). 숫자 두 칸과 숫자로 시작하는 셋째 칸이다.
fn version_like(name: &str) -> bool {
    let mut parts = name.splitn(3, '.');
    let digits =
        |part: Option<&str>| part.is_some_and(|text| !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()));
    digits(parts.next())
        && digits(parts.next())
        && parts.next().is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()))
}

/// 실행본 폴더인가. 이름이 판 번호이고 `bin` 아래에 두 실행 파일이 링크가 아닌 파일로 있어야 한다.
fn is_runtime(path: &Path) -> bool {
    let bin = path.join("bin");
    let plain_file = |name: &&str| fs::symlink_metadata(bin.join(name)).is_ok_and(|meta| meta.is_file());
    path.file_name().and_then(|name| name.to_str()).is_some_and(version_like)
        && is_real_dir(path)
        && is_real_dir(&bin)
        && RUNTIME_BINARIES.iter().all(plain_file)
}

/// 데이터 폴더 바로 아래의 실행본 폴더, 이름순. `backup`, `logs`와 남의 폴더는 고르지 않는다.
fn runtimes_in(home: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(home) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> =
        entries.filter_map(Result::ok).map(|entry| entry.path()).filter(|path| is_runtime(path)).collect();
    found.sort();
    found
}

/// 링크가 데이터 폴더 안을 가리키는가. 대상이 이미 없어도 적힌 경로로 판단한다.
fn points_into(link: &Path, home: &Path) -> bool {
    let Ok(target) = fs::read_link(link) else {
        return false;
    };
    let target =
        if target.is_absolute() { target } else { link.parent().map_or(target.clone(), |dir| dir.join(&target)) };
    let real_home = fs::canonicalize(home).unwrap_or_else(|_| home.to_path_buf());
    target.starts_with(home) || target.starts_with(&real_home)
}

/// 실행본을 지울 계획. `user_home`은 `~/.local/bin/kairos`를 찾을 홈 폴더다.
pub fn plan_runtime_removal(home: &Path, user_home: &Path) -> RemovalPlan {
    let home = &settled(home);
    let current = home.join(CURRENT_LINK);
    let command = user_home.join(".local").join("bin").join("kairos");
    RemovalPlan {
        runtimes: runtimes_in(home),
        current: fs::symlink_metadata(&current).is_ok_and(|meta| meta.file_type().is_symlink()).then_some(current),
        command_link: points_into(&command, home).then_some(command),
    }
}

/// 계획대로 지운다. 지운 것을 줄로 돌려준다. 하나가 실패하면 거기서 멈춘다.
pub fn remove_runtime(plan: &RemovalPlan) -> KairosResult<Vec<String>> {
    let mut lines = Vec::new();
    // 링크를 먼저 지운다. 중간에 멈춰도 없는 실행본을 가리키는 명령이 남지 않는다.
    for link in plan.command_link.iter().chain(plan.current.iter()) {
        remove_if_present(link)?;
        lines.push(format!("지움 {}", link.display()));
    }
    for runtime in &plan.runtimes {
        if is_real_dir(runtime) {
            fs::remove_dir_all(runtime)?;
        }
        lines.push(format!("지움 {}", runtime.display()));
    }
    Ok(lines)
}

/// 데이터 폴더의 자리가 지우기에 맞는지 본다. 맞으면 링크를 푼 경로를 돌려준다.
fn checked_place(home: &Path, user_home: &Path) -> Result<PathBuf, &'static str> {
    let home = settled(home);
    // `<링크>/<하위>/..`는 끝 성분이 폴더라 링크 검사를 지나간다. 받지 않는다.
    if home.components().any(|part| part == Component::ParentDir) {
        return Err("데이터 폴더 경로에 ..가 있습니다. KAIROS_HOME을 확인하세요");
    }
    let Ok(real) = fs::canonicalize(&home) else {
        return Err("데이터 폴더가 없습니다");
    };
    if !is_real_dir(&home) {
        return Err("데이터 폴더가 링크입니다. 직접 지우세요");
    }
    // 홈 폴더를 모르면 홈 폴더인지도 가릴 수 없다.
    let Ok(real_user) = fs::canonicalize(user_home) else {
        return Err("홈 폴더를 확인하지 못했습니다. HOME을 확인하세요");
    };
    // 홈 폴더, 그 조상, 루트는 kairos 표지가 있어도 지우지 않는다.
    if real.parent().is_none() || real_user.starts_with(&real) {
        return Err("데이터 폴더가 홈 폴더이거나 그 위입니다. KAIROS_HOME을 확인하세요");
    }
    Ok(real)
}

/// 실행본을 지우면 안 되는 까닭. 지워도 되면 None이다. 없는 데이터 폴더는 지울 실행본이 없을 뿐이라 막지 않는다.
pub fn runtime_refusal(home: &Path, user_home: &Path) -> Option<&'static str> {
    if fs::symlink_metadata(settled(home)).is_err() {
        return None;
    }
    checked_place(home, user_home).err()
}

/// 데이터 폴더 전체를 지우면 안 되는 까닭. 지워도 되면 None이다.
pub fn purge_refusal(home: &Path, user_home: &Path) -> Option<&'static str> {
    let real = match checked_place(home, user_home) {
        Ok(real) => real,
        Err(reason) => return Some(reason),
    };
    if !HOME_MARKERS.iter().any(|name| fs::symlink_metadata(real.join(name)).is_ok()) {
        return Some("kairos 데이터 폴더로 보이지 않습니다(kairos.db와 연결 설정이 없음). 직접 지우세요");
    }
    None
}

/// 데이터 폴더를 통째로 지운다. 먼저 `purge_refusal`로 확인하고, 확인한 경로(링크를 푼 경로)를 지운다.
pub fn remove_data(home: &Path, user_home: &Path) -> KairosResult<()> {
    if purge_refusal(home, user_home).is_some() {
        return Err(kairos_core::KairosError::new("purge_refused"));
    }
    fs::remove_dir_all(fs::canonicalize(settled(home))?)?;
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::symlink;

    use super::*;

    /// `<임시>/user/data`에 실행본 둘, 다른 폴더들, `current`, `~/.local/bin/kairos`를 둔다.
    fn installed() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let user = root.join("user");
        let home = user.join("data");
        for version in ["0.1.0", "0.2.0"] {
            fs::create_dir_all(home.join(version).join("bin")).unwrap();
            for name in RUNTIME_BINARIES {
                fs::write(home.join(version).join("bin").join(name), "binary").unwrap();
            }
            fs::create_dir_all(home.join(version).join("ops/notifier/KairosNotifier.app")).unwrap();
            fs::create_dir_all(home.join(version).join("plugins/orca-panel")).unwrap();
        }
        for kept in ["backup", "logs"] {
            fs::create_dir_all(home.join(kept)).unwrap();
        }
        fs::write(home.join("kairos.db"), "db").unwrap();
        symlink("0.2.0", home.join("current")).unwrap();
        fs::create_dir_all(user.join(".local/bin")).unwrap();
        symlink(home.join("current/bin/kairos"), user.join(".local/bin/kairos")).unwrap();
        (dir, user, home)
    }

    #[test]
    fn 실행본과_링크만_고르고_데이터는_남긴다() {
        let (_dir, user, home) = installed();
        let plan = plan_runtime_removal(&home, &user);
        assert_eq!(plan.runtimes, [home.join("0.1.0"), home.join("0.2.0")]);
        assert_eq!(plan.current, Some(home.join("current")));
        assert_eq!(plan.command_link, Some(user.join(".local/bin/kairos")));
        assert_eq!(plan.notifier_apps().len(), 2);
        assert_eq!(plan.panel_dirs(), [home.join("0.1.0/plugins/orca-panel"), home.join("0.2.0/plugins/orca-panel")]);
        let lines = remove_runtime(&plan).unwrap();
        assert_eq!(lines.len(), 4);
        assert!(!home.join("0.2.0").exists() && !home.join("0.1.0").exists());
        assert!(fs::symlink_metadata(home.join("current")).is_err());
        assert!(fs::symlink_metadata(user.join(".local/bin/kairos")).is_err());
        for kept in ["backup", "logs", "kairos.db"] {
            assert!(home.join(kept).exists(), "{kept}");
        }
        assert!(plan_runtime_removal(&home, &user).is_empty());
    }

    #[test]
    fn 남의_명령과_링크로_건_실행본은_건드리지_않는다() {
        let (_dir, user, home) = installed();
        let command = user.join(".local/bin/kairos");
        // 링크가 아닌 파일.
        fs::remove_file(&command).unwrap();
        fs::write(&command, "someone else").unwrap();
        assert_eq!(plan_runtime_removal(&home, &user).command_link, None);
        // 다른 곳을 가리키는 링크.
        fs::remove_file(&command).unwrap();
        symlink("/usr/bin/true", &command).unwrap();
        assert_eq!(plan_runtime_removal(&home, &user).command_link, None);
        // 실행본처럼 보이는 링크는 따라가지 않는다.
        let outside = user.join("outside");
        fs::create_dir_all(outside.join("bin")).unwrap();
        for name in RUNTIME_BINARIES {
            fs::write(outside.join("bin").join(name), "binary").unwrap();
        }
        symlink(&outside, home.join("9.9.9")).unwrap();
        let plan = plan_runtime_removal(&home, &user);
        assert!(!plan.runtimes.contains(&home.join("9.9.9")), "{:?}", plan.runtimes);
        remove_runtime(&plan).unwrap();
        assert!(outside.join("bin/kairos").is_file());
    }

    #[test]
    fn 데이터_폴더는_표지가_있고_홈_폴더_밖이_아닐_때만_지운다() {
        let (_dir, user, home) = installed();
        assert_eq!(purge_refusal(&home, &user), None);
        // 홈 폴더와 그 조상.
        fs::write(user.join("kairos.db"), "db").unwrap();
        assert!(purge_refusal(&user, &user).is_some());
        assert!(purge_refusal(user.parent().unwrap(), &user).is_some());
        assert!(purge_refusal(Path::new("/"), &user).is_some());
        // 표지가 없는 폴더, 없는 폴더, 링크.
        let other = user.join("documents");
        fs::create_dir_all(&other).unwrap();
        assert!(purge_refusal(&other, &user).is_some());
        assert!(purge_refusal(&user.join("missing"), &user).is_some());
        symlink(&home, user.join("alias")).unwrap();
        assert!(purge_refusal(&user.join("alias"), &user).is_some());
        // 끝에 `/`를 붙여도 링크는 링크다.
        let slashed = PathBuf::from(format!("{}/", user.join("alias").display()));
        assert!(purge_refusal(&slashed, &user).is_some());
        assert!(remove_data(&slashed, &user).is_err());
        assert!(home.join("kairos.db").is_file());
        // 다른 도구도 쓰는 이름은 표지가 아니다.
        fs::write(other.join("config.toml"), "x").unwrap();
        symlink("somewhere", other.join("current")).unwrap();
        assert!(purge_refusal(&other, &user).is_some());
        // `..`로 링크를 지나가는 경로도 받지 않는다.
        let dotted = user.join("alias").join("logs").join("..");
        assert!(purge_refusal(&dotted, &user).is_some());
        // 홈 폴더를 모르면 지우지 않는다.
        assert!(purge_refusal(&home, Path::new("")).is_some());
        assert!(remove_data(&other, &user).is_err());
        assert!(other.exists());

        remove_data(&home, &user).unwrap();
        assert!(!home.exists());
        assert!(user.join("documents").exists());
    }

    #[test]
    fn 판_번호_이름과_두_실행_파일이_있는_폴더만_실행본이다() {
        let (_dir, user, home) = installed();
        // 이름은 판 번호지만 kairos-run이 없다. 이름이 판 번호가 아니다.
        for (name, files) in
            [("1.0.0", &["kairos"][..]), ("tools", &RUNTIME_BINARIES[..]), ("1.0", &RUNTIME_BINARIES[..])]
        {
            fs::create_dir_all(home.join(name).join("bin")).unwrap();
            for file in files {
                fs::write(home.join(name).join("bin").join(file), "binary").unwrap();
            }
        }
        fs::create_dir_all(home.join("0.3.0-rc.1/bin")).unwrap();
        for name in RUNTIME_BINARIES {
            fs::write(home.join("0.3.0-rc.1/bin").join(name), "binary").unwrap();
        }
        let plan = plan_runtime_removal(&home, &user);
        assert_eq!(plan.runtimes, [home.join("0.1.0"), home.join("0.2.0"), home.join("0.3.0-rc.1")]);
        assert!(version_like("10.20.3") && !version_like(".local") && !version_like("0.2.x") && !version_like("0..1"));
    }

    #[test]
    fn 홈_폴더를_데이터_폴더로_주면_실행본도_지우지_않는다() {
        let (_dir, user, home) = installed();
        assert_eq!(runtime_refusal(&home, &user), None);
        // 없는 데이터 폴더는 막지 않는다. 지울 실행본이 없을 뿐이다.
        assert_eq!(runtime_refusal(&user.join("missing"), &user), None);
        assert!(plan_runtime_removal(&user.join("missing"), &user).runtimes.is_empty());
        // 홈 폴더가 데이터 폴더다. ~/.local/bin/kairos 링크 때문에 ~/.local이 실행본으로 보이면 안 된다.
        assert!(runtime_refusal(&user, &user).is_some());
        assert!(runtime_refusal(user.parent().unwrap(), &user).is_some());
        assert!(plan_runtime_removal(&user, &user).runtimes.is_empty());
        symlink(&home, user.join("alias")).unwrap();
        assert!(runtime_refusal(&user.join("alias"), &user).is_some());
        assert!(runtime_refusal(&PathBuf::from(format!("{}/", user.join("alias").display())), &user).is_some());
    }
}
