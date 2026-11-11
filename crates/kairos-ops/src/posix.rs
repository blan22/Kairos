//! `node:path`의 POSIX 동작. 경로 검사가 문자열 비교라서 `std::path`로 바꾸면 판정이 달라진다.
//! `Path`는 비교할 때 `//`와 끝의 `/`를 지우고, `PathBuf::join`은 `..`을 풀지 않는다.

/// `path.dirname`.
pub(crate) fn dirname(path: &str) -> &str {
    let bytes = path.as_bytes();
    let Some(first) = bytes.first() else {
        return ".";
    };
    let has_root = *first == b'/';
    let mut matched_slash = true;
    for index in (1..bytes.len()).rev() {
        if bytes[index] != b'/' {
            matched_slash = false;
        } else if !matched_slash {
            return if has_root && index == 1 { "//" } else { &path[..index] };
        }
    }
    if has_root { "/" } else { "." }
}

/// `path.basename`.
pub(crate) fn basename(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    trimmed.rsplit('/').next().unwrap_or(trimmed)
}

/// `path.normalize`.
pub(crate) fn normalize(path: &str) -> String {
    if path.is_empty() {
        return ".".to_owned();
    }
    let absolute = path.starts_with('/');
    let trailing = path.ends_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." if parts.last().is_some_and(|last| *last != "..") => {
                parts.pop();
            }
            ".." if absolute => {}
            other => parts.push(other),
        }
    }
    let joined = parts.join("/");
    match (joined.is_empty(), absolute, trailing) {
        (true, true, _) => "/".to_owned(),
        (true, false, true) => "./".to_owned(),
        (true, false, false) => ".".to_owned(),
        (false, true, true) => format!("/{joined}/"),
        (false, true, false) => format!("/{joined}"),
        (false, false, true) => format!("{joined}/"),
        (false, false, false) => joined,
    }
}

/// `path.join`.
pub(crate) fn join(parts: &[&str]) -> String {
    let filled: Vec<&str> = parts.iter().copied().filter(|part| !part.is_empty()).collect();
    if filled.is_empty() { ".".to_owned() } else { normalize(&filled.join("/")) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dirname은_node와_같다() {
        for (path, expected) in [
            ("", "."),
            ("a", "."),
            ("/", "/"),
            ("/a", "/"),
            ("/a/b", "/a"),
            ("/a/b/", "/a"),
            ("/a//b", "/a/"),
            ("a/b", "a"),
            ("//a", "//"),
        ] {
            assert_eq!(dirname(path), expected, "{path}");
        }
    }

    #[test]
    fn basename은_node와_같다() {
        for (path, expected) in [("", ""), ("/", ""), ("a", "a"), ("/a/b", "b"), ("/a/b/", "b"), ("/a/b//", "b")] {
            assert_eq!(basename(path), expected, "{path}");
        }
    }

    #[test]
    fn normalize는_node와_같다() {
        for (path, expected) in [
            ("", "."),
            ("/", "/"),
            ("//a", "/a"),
            ("/a/./b", "/a/b"),
            ("/a/../b", "/b"),
            ("/../a", "/a"),
            ("/a/b/", "/a/b/"),
            ("a/../..", ".."),
            ("a/..", "."),
            ("a/../", "./"),
            ("../../a", "../../a"),
        ] {
            assert_eq!(normalize(path), expected, "{path}");
        }
    }

    #[test]
    fn join은_빈_조각을_버리고_정규화한다() {
        assert_eq!(join(&["/a/", "", "b"]), "/a/b");
        assert_eq!(join(&["/a/b", "..", "c"]), "/a/c");
        assert_eq!(join(&["", ""]), ".");
    }
}
