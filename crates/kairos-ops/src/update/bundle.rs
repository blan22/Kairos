//! 새 판 묶음을 받아 확인하고 푼다. `kairos update --apply`만 쓴다.
//!
//! 순서는 받기(gh release download), SHA256, 빌드 증명(gh attestation verify), 풀기(tar), 모양 검사다.
//! 하나라도 틀리면 그 자리에서 멈춘다. 받은 파일은 호출한 쪽이 고른 작업 폴더에만 둔다.
//! SHA256은 같은 릴리스에서 받으므로 전송 중 손상만 거른다. 올린 사람은 빌드 증명이 확인한다.
//! 증명은 이 저장소의 릴리스 워크플로가 그 판의 태그에서 GitHub 러너로 만든 묶음만 통과한다.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use kairos_core::platform::Os;
use kairos_core::process::{ProcessRequest, ProcessRunner};
use kairos_core::{KairosError, KairosResult};
use sha2::{Digest, Sha256};

use super::{RELEASE_REPO, Version, gh_env};

/// 묶음을 만들고 증명을 남기는 워크플로.
pub const RELEASE_WORKFLOW: &str = ".github/workflows/release.yml";
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600);
const VERIFY_TIMEOUT: Duration = Duration::from_secs(120);
const EXTRACT_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_OUTPUT: usize = 64 * 1024;
const MAX_SUM_BYTES: u64 = 1024;
const SHA256_HEX: usize = 64;

/// OS와 CPU에 맞는 릴리스 묶음 이름. 릴리스 워크플로가 올리는 이름과 같다. 없으면 None이다.
pub fn asset_name(os: Os, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        (Os::MacOs, "aarch64") => Some("kairos-aarch64-apple-darwin.tar.gz"),
        (Os::Linux, "x86_64") => Some("kairos-x86_64-unknown-linux-gnu.tar.gz"),
        (Os::Linux, "aarch64") => Some("kairos-aarch64-unknown-linux-gnu.tar.gz"),
        (Os::Windows, "x86_64") => Some("kairos-x86_64-pc-windows-msvc.zip"),
        _ => None,
    }
}

/// 묶음 안의 kairos 실행 파일 이름.
const fn kairos_exe(os: Os) -> &'static str {
    match os {
        Os::Windows => "kairos.exe",
        Os::MacOs | Os::Linux => "kairos",
    }
}

fn fail(code: &str) -> KairosError {
    KairosError::new(code)
}

/// 묶음을 받아 확인하고 푸는 일.
pub struct BundleFetcher {
    runner: Arc<dyn ProcessRunner>,
    gh: String,
    tar: String,
    env: BTreeMap<String, String>,
    os: Os,
    arch: String,
}

impl BundleFetcher {
    /// `env`는 프로세스 환경이다. gh에는 `gh_env`로 고른 값만 넘긴다. tar도 같은 환경을 쓴다.
    pub fn new(
        runner: Arc<dyn ProcessRunner>,
        gh: impl Into<String>,
        tar: impl Into<String>,
        env: &BTreeMap<String, String>,
        os: Os,
    ) -> Self {
        Self { runner, gh: gh.into(), tar: tar.into(), env: gh_env(env), os, arch: std::env::consts::ARCH.to_owned() }
    }

    /// 받고 확인하고 풀어 묶음의 맨 위 폴더(`kairos-<판>`)를 돌려준다. `work`는 비어 있는 작업 폴더다.
    pub fn fetch(&self, version: Version, work: &Path) -> KairosResult<PathBuf> {
        let asset = asset_name(self.os, &self.arch).ok_or_else(|| fail("update_unsupported_platform"))?;
        let download = work.join("download");
        fs::create_dir(&download)?;
        self.run(&self.gh, Self::download_args(version, asset, &download), DOWNLOAD_TIMEOUT, "update_download_failed")?;
        let file = download.join(asset);
        verify_checksum(&file, &download.join(format!("{asset}.sha256")))?;
        self.run(&self.gh, Self::attestation_args(version, &file), VERIFY_TIMEOUT, "update_attestation_failed")?;
        let bundle = work.join("bundle");
        fs::create_dir(&bundle)?;
        self.run(&self.tar, self.extract_args(&file, &bundle), EXTRACT_TIMEOUT, "update_extract_failed")?;
        bundle_root(&bundle, version, self.os)
    }

    fn run(&self, program: &str, args: Vec<String>, timeout: Duration, code: &str) -> KairosResult<()> {
        let request = ProcessRequest::new(program, args, timeout).env(self.env.clone()).max_output(MAX_OUTPUT);
        if self.runner.run(&request).success() { Ok(()) } else { Err(fail(code)) }
    }

    /// 그 판의 묶음과 SHA256 파일만 받는다.
    pub fn download_args(version: Version, asset: &str, dir: &Path) -> Vec<String> {
        vec![
            "release".to_owned(),
            "download".to_owned(),
            format!("v{version}"),
            "--repo".to_owned(),
            RELEASE_REPO.to_owned(),
            "--pattern".to_owned(),
            asset.to_owned(),
            "--pattern".to_owned(),
            format!("{asset}.sha256"),
            "--dir".to_owned(),
            dir.to_string_lossy().into_owned(),
            "--clobber".to_owned(),
        ]
    }

    /// 이 저장소의 릴리스 워크플로가 그 판의 태그에서 GitHub 러너로 만든 묶음인지 본다.
    pub fn attestation_args(version: Version, file: &Path) -> Vec<String> {
        vec![
            "attestation".to_owned(),
            "verify".to_owned(),
            file.to_string_lossy().into_owned(),
            "--repo".to_owned(),
            RELEASE_REPO.to_owned(),
            // 서명 인증서의 신원이 그 판의 태그에서 돈 release.yml과 정확히 같아야 한다.
            // `--signer-workflow`는 앞부분만 맞춰 보므로 이름이 release.yml로 시작하는 다른 워크플로도 통과한다.
            "--cert-identity".to_owned(),
            format!("https://github.com/{RELEASE_REPO}/{RELEASE_WORKFLOW}@refs/tags/v{version}"),
            "--source-ref".to_owned(),
            format!("refs/tags/v{version}"),
            "--deny-self-hosted-runners".to_owned(),
        ]
    }

    /// bsdtar(macOS, Windows의 tar.exe)는 zip도 푼다. 절대 경로와 `..`가 든 항목은 tar가 풀지 않는다.
    fn extract_args(&self, file: &Path, dir: &Path) -> Vec<String> {
        let mode = if self.os == Os::Windows { "-xf" } else { "-xzf" };
        vec![mode.to_owned(), file.to_string_lossy().into_owned(), "-C".to_owned(), dir.to_string_lossy().into_owned()]
    }
}

/// SHA256 파일의 첫 낱말이 묶음의 SHA256과 같은가.
fn verify_checksum(file: &Path, sum_file: &Path) -> KairosResult<()> {
    let small = fs::metadata(sum_file).is_ok_and(|meta| meta.is_file() && meta.len() <= MAX_SUM_BYTES);
    let text =
        small.then(|| fs::read_to_string(sum_file).ok()).flatten().ok_or_else(|| fail("update_checksum_missing"))?;
    let expected = text.split_whitespace().next().unwrap_or("");
    let shaped =
        expected.len() == SHA256_HEX && expected.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    if !shaped {
        return Err(fail("update_checksum_missing"));
    }
    let mut hasher = Sha256::new();
    io::copy(&mut File::open(file).map_err(|_| fail("update_download_failed"))?, &mut hasher)?;
    let actual: String = hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
    if actual == expected { Ok(()) } else { Err(fail("update_checksum_mismatch")) }
}

/// 푼 폴더에 `kairos-<판>` 하나만 있고, 그 안에 실행 파일과 같은 판의 VERSION이 있어야 한다.
/// 실행 파일은 링크가 아닌 일반 파일이어야 한다.
fn bundle_root(bundle: &Path, version: Version, os: Os) -> KairosResult<PathBuf> {
    let entries: Vec<_> = fs::read_dir(bundle)?.collect::<Result<_, _>>()?;
    let [entry] = entries.as_slice() else {
        return Err(fail("update_bundle_invalid"));
    };
    let root = entry.path();
    let named = entry.file_name().to_str() == Some(format!("kairos-{version}").as_str());
    let is_dir = fs::symlink_metadata(&root).is_ok_and(|meta| meta.is_dir());
    // `bin`도 링크면 안 된다. `symlink_metadata`는 마지막 조각의 링크만 따라가지 않는다.
    let bin = root.join("bin");
    let bin_dir = fs::symlink_metadata(&bin).is_ok_and(|meta| meta.is_dir());
    let exe = bin.join(kairos_exe(os));
    let is_exe = bin_dir && fs::symlink_metadata(&exe).is_ok_and(|meta| meta.is_file());
    let stamped = fs::read_to_string(root.join("VERSION")).is_ok_and(|text| Version::parse(&text) == Some(version));
    if named && is_dir && is_exe && stamped { Ok(root) } else { Err(fail("update_bundle_invalid")) }
}

#[cfg(all(test, unix))]
mod tests {
    use std::sync::Mutex;

    use kairos_core::process::{ProcessOutput, SystemProcessRunner};

    use super::*;

    const VERSION: Version = Version::new(9, 1, 0);

    /// gh 역할을 한다. 받기는 미리 만든 파일을 `--dir`에 놓고, 증명은 정한 코드로 답한다. tar는 진짜를 부른다.
    struct Fake {
        files: Vec<(String, Vec<u8>)>,
        attestation: i32,
        calls: Mutex<Vec<Vec<String>>>,
    }

    impl ProcessRunner for Fake {
        fn run(&self, request: &ProcessRequest) -> ProcessOutput {
            self.calls.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(request.args.clone());
            match request.args.first().map(String::as_str) {
                Some("release") => {
                    let dir = request.args.iter().skip_while(|arg| *arg != "--dir").nth(1).cloned().unwrap_or_default();
                    for (name, bytes) in &self.files {
                        let _ = fs::write(Path::new(&dir).join(name), bytes);
                    }
                    ProcessOutput::exited(0, "")
                }
                Some("attestation") => ProcessOutput::exited(self.attestation, ""),
                _ => SystemProcessRunner.run(request),
            }
        }
    }

    fn sha256(bytes: &[u8]) -> String {
        Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
    }

    /// `kairos-<판>/bin/kairos`와 VERSION이 든 tar.gz.
    fn tarball(dir: &Path, top: &str, stamp: &str) -> Vec<u8> {
        let stage = dir.join("stage");
        let root = stage.join(top);
        fs::create_dir_all(root.join("bin")).unwrap_or_else(|error| panic!("{error}"));
        fs::write(root.join("bin").join("kairos"), "#!/bin/sh\n").unwrap_or_else(|error| panic!("{error}"));
        fs::write(root.join("VERSION"), stamp).unwrap_or_else(|error| panic!("{error}"));
        let out = dir.join("out.tar.gz");
        let made = SystemProcessRunner.run(&ProcessRequest::new(
            "/usr/bin/tar",
            ["-czf", &out.to_string_lossy(), "-C", &stage.to_string_lossy(), top],
            Duration::from_secs(30),
        ));
        assert!(made.success(), "{}", made.stderr);
        let bytes = fs::read(&out).unwrap_or_default();
        let _ = fs::remove_dir_all(&stage);
        bytes
    }

    fn fetcher(fake: Arc<Fake>) -> BundleFetcher {
        let os = Os::current();
        let mut fetcher = BundleFetcher::new(fake, "/synthetic/gh", "/usr/bin/tar", &BTreeMap::new(), os);
        // 시험 러너의 CPU와 상관없이 같은 묶음 이름을 쓴다.
        fetcher.arch = if os == Os::MacOs { "aarch64" } else { "x86_64" }.to_owned();
        fetcher
    }

    fn asset() -> &'static str {
        let os = Os::current();
        asset_name(os, if os == Os::MacOs { "aarch64" } else { "x86_64" }).unwrap_or("")
    }

    fn setup(attestation: i32, sum: Option<String>, top: &str, stamp: &str) -> (tempfile::TempDir, Arc<Fake>) {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let bytes = tarball(dir.path(), top, stamp);
        let sum = sum.unwrap_or_else(|| format!("{}  {}\n", sha256(&bytes), asset()));
        let files = vec![(asset().to_owned(), bytes), (format!("{}.sha256", asset()), sum.into_bytes())];
        fs::create_dir(dir.path().join("work")).unwrap_or_else(|error| panic!("{error}"));
        (dir, Arc::new(Fake { files, attestation, calls: Mutex::new(Vec::new()) }))
    }

    #[test]
    fn 받고_확인하고_풀어_묶음_폴더를_돌려준다() {
        let (dir, fake) = setup(0, None, "kairos-9.1.0", "9.1.0\n");
        let work = dir.path().join("work");
        let root = fetcher(fake.clone()).fetch(VERSION, &work).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(root, work.join("bundle").join("kairos-9.1.0"));
        let calls = fake.calls.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        assert_eq!(calls[0][..3], ["release", "download", "v9.1.0"]);
        assert!(calls[0].windows(2).any(|pair| pair == ["--repo", "blan22/Kairos"]), "{:?}", calls[0]);
        assert_eq!(calls[1][..2], ["attestation", "verify"]);
        for pair in [
            ["--cert-identity", "https://github.com/blan22/Kairos/.github/workflows/release.yml@refs/tags/v9.1.0"],
            ["--source-ref", "refs/tags/v9.1.0"],
        ] {
            assert!(calls[1].windows(2).any(|found| found == pair), "{:?}", calls[1]);
        }
        assert!(calls[1].contains(&"--deny-self-hosted-runners".to_owned()));
    }

    #[test]
    fn 증명이_없거나_sha256이_다르면_풀지_않는다() {
        let (dir, fake) = setup(1, None, "kairos-9.1.0", "9.1.0");
        let error = fetcher(fake.clone()).fetch(VERSION, &dir.path().join("work")).unwrap_err();
        assert_eq!(error.code(), "update_attestation_failed");
        assert!(!dir.path().join("work").join("bundle").exists());

        let wrong = Some(format!("{}  x\n", "0".repeat(64)));
        let (dir, fake) = setup(0, wrong, "kairos-9.1.0", "9.1.0");
        let error = fetcher(fake.clone()).fetch(VERSION, &dir.path().join("work")).unwrap_err();
        assert_eq!(error.code(), "update_checksum_mismatch");
        // 증명은 묻지도 않는다.
        assert_eq!(fake.calls.lock().unwrap_or_else(std::sync::PoisonError::into_inner).len(), 1);

        for bad in ["", "not-a-sum", &"A".repeat(64)] {
            let (dir, fake) = setup(0, Some(bad.to_owned()), "kairos-9.1.0", "9.1.0");
            let error = fetcher(fake).fetch(VERSION, &dir.path().join("work")).unwrap_err();
            assert_eq!(error.code(), "update_checksum_missing", "{bad:?}");
        }
    }

    #[test]
    fn 묶음의_모양이나_판이_다르면_쓰지_않는다() {
        for (top, stamp) in [("kairos-9.0.0", "9.1.0"), ("kairos-9.1.0", "9.0.0"), ("other", "9.1.0")] {
            let (dir, fake) = setup(0, None, top, stamp);
            let error = fetcher(fake).fetch(VERSION, &dir.path().join("work")).unwrap_err();
            assert_eq!(error.code(), "update_bundle_invalid", "{top} {stamp}");
        }
    }

    #[test]
    fn bin_폴더가_링크인_묶음은_쓰지_않는다() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let root = dir.path().join("bundle").join("kairos-9.1.0");
        let elsewhere = dir.path().join("elsewhere");
        fs::create_dir_all(&root).unwrap_or_else(|error| panic!("{error}"));
        fs::create_dir_all(&elsewhere).unwrap_or_else(|error| panic!("{error}"));
        fs::write(elsewhere.join("kairos"), "#!/bin/sh\n").unwrap_or_else(|error| panic!("{error}"));
        fs::write(root.join("VERSION"), "9.1.0").unwrap_or_else(|error| panic!("{error}"));
        std::os::unix::fs::symlink(&elsewhere, root.join("bin")).unwrap_or_else(|error| panic!("{error}"));
        let error = bundle_root(&dir.path().join("bundle"), VERSION, Os::MacOs).unwrap_err();
        assert_eq!(error.code(), "update_bundle_invalid");
    }

    #[test]
    fn 묶음_이름은_릴리스_워크플로와_같다() {
        assert_eq!(asset_name(Os::MacOs, "aarch64"), Some("kairos-aarch64-apple-darwin.tar.gz"));
        assert_eq!(asset_name(Os::MacOs, "x86_64"), None);
        assert_eq!(asset_name(Os::Linux, "aarch64"), Some("kairos-aarch64-unknown-linux-gnu.tar.gz"));
        assert_eq!(asset_name(Os::Windows, "x86_64"), Some("kairos-x86_64-pc-windows-msvc.zip"));
        let workflow = include_str!("../../../../.github/workflows/release.yml");
        for (os, arch) in [(Os::MacOs, "aarch64"), (Os::Linux, "x86_64"), (Os::Windows, "x86_64")] {
            let name = asset_name(os, arch).unwrap_or("");
            let stem = name.replace("x86_64", "${{ matrix.arch }}").replace("aarch64", "${{ matrix.arch }}");
            assert!(workflow.contains(name) || workflow.contains(&stem), "{name}");
        }
    }
}
