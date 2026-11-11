//! 표준 입출력. 명령은 이 객체로만 찍고 읽는다. 시험은 출력을 모으고 입력을 넣는다.

use std::io::{self, BufRead, Read, Write};
use std::sync::{Arc, Mutex, PoisonError};

use kairos_core::KairosResult;
use serde::Serialize;

#[derive(Debug, Default)]
struct Captured {
    out: String,
    err: String,
    input: String,
}

/// 파이프가 닫혀도 패닉하지 않는다. `println!`은 받는 쪽이 사라지면 패닉한다.
#[derive(Clone, Debug, Default)]
pub struct Console {
    captured: Option<Arc<Mutex<Captured>>>,
}

impl Console {
    /// 표준 입출력에 붙은 콘솔.
    pub fn system() -> Self {
        Self::default()
    }

    /// 시험용. 입력은 `input`이고 출력은 `out`, `err`로 읽는다.
    pub fn captured(input: impl Into<String>) -> Self {
        let captured = Captured { input: input.into(), ..Captured::default() };
        Self { captured: Some(Arc::new(Mutex::new(captured))) }
    }

    fn with_captured<T>(&self, read: impl FnOnce(&mut Captured) -> T) -> Option<T> {
        self.captured.as_ref().map(|captured| read(&mut captured.lock().unwrap_or_else(PoisonError::into_inner)))
    }

    pub fn out(&self) -> String {
        self.with_captured(|captured| captured.out.clone()).unwrap_or_default()
    }

    pub fn err(&self) -> String {
        self.with_captured(|captured| captured.err.clone()).unwrap_or_default()
    }

    /// 줄 하나를 표준 출력에 찍는다.
    pub fn print(&self, line: &str) {
        if self.with_captured(|captured| captured.out.push_str(&format!("{line}\n"))).is_none() {
            let _ = writeln!(io::stdout(), "{line}");
        }
    }

    /// 줄 하나를 표준 오류에 찍는다.
    pub fn error(&self, line: &str) {
        if self.with_captured(|captured| captured.err.push_str(&format!("{line}\n"))).is_none() {
            let _ = writeln!(io::stderr(), "{line}");
        }
    }

    /// `JSON.stringify` 한 줄.
    pub fn emit<T: Serialize>(&self, value: &T) -> KairosResult<()> {
        let text = serde_json::to_string(value).map_err(|_| kairos_core::KairosError::new("json_error"))?;
        self.print(&text);
        Ok(())
    }

    /// 줄바꿈 없이 묻는 말을 찍는다. 대화형 명령(`setup`)이 쓴다.
    pub fn prompt(&self, text: &str) {
        if self.with_captured(|captured| captured.out.push_str(&format!("{text}\n"))).is_none() {
            let mut stdout = io::stdout();
            let _ = write!(stdout, "{text} ");
            let _ = stdout.flush();
        }
    }

    /// 표준 입력에서 한 줄을 읽는다. 끝이면 None이다. 줄 끝의 개행은 뗀다.
    pub fn read_line(&self) -> KairosResult<Option<String>> {
        // 시험 콘솔은 넣은 입력이 끝이다. 실제 표준 입력으로 넘어가지 않는다.
        let captured = self.with_captured(|captured| {
            if captured.input.is_empty() {
                return None;
            }
            let end = captured.input.find('\n').map_or(captured.input.len(), |at| at + 1);
            let line: String = captured.input.drain(..end).collect();
            Some(line.trim_end_matches(['\n', '\r']).to_owned())
        });
        if self.captured.is_some() {
            return Ok(captured.flatten());
        }
        let mut line = String::new();
        if io::stdin().lock().read_line(&mut line)? == 0 {
            return Ok(None);
        }
        Ok(Some(line.trim_end_matches(['\n', '\r']).to_owned()))
    }

    /// 표준 입력을 끝까지 읽는다. `Buffer.toString('utf8')`처럼 깨진 바이트는 U+FFFD가 된다.
    pub fn read_input(&self) -> KairosResult<String> {
        if let Some(input) = self.with_captured(|captured| captured.input.clone()) {
            return Ok(input);
        }
        let mut bytes = Vec::new();
        io::stdin().read_to_end(&mut bytes)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
}
