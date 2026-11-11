// KairosNotifier. kairos 알림을 macOS 알림 센터로 띄우고, 누르면 kairos reveal로 넘긴다.
// LSUIElement 앱이다. Dock에 뜨지 않고 창도 없다.
//
//   KairosNotifier post '<json>'  알림 하나를 띄운다. 권한을 묻지 않는다.
//   KairosNotifier setup          권한을 묻고 허용되면 안내 알림을 띄운다. 사용자가 한 번 실행한다.
//   KairosNotifier status         권한 상태를 출력한다.
//   KairosNotifier                알림을 누르면 macOS가 인자 없이 띄운다. 응답을 받아 kairos reveal을 부른다.
//
// 종료 코드: 0 게시, 2 입력 오류, 3 권한 없음 또는 미결정, 4 게시 실패.
// 실행 파일 경로는 번들 위치와 홈 폴더로만 정한다. 알림 내용과 defaults에서 받지 않는다.
// 입력 검사는 Payload.swift에 있다.
import AppKit
import Foundation
import OSLog
import UserNotifications
import os

enum Timeout {
    /// post가 끝나야 하는 시간이다. 프로세스 시작부터 잰다. kairos의 제한(5초)보다 짧게 잡아 kairos가 죽이기 전에 4로 끝낸다.
    /// 시간 초과 때 알림 센터가 요청을 이미 받았을 수 있다. kairos는 4를 받으면 다음 틱에 다시 보낸다. 같은 알림이 한 번 더 뜰 수 있지만 알림을 잃는 것보다 낫다고 본다.
    static let post: TimeInterval = 3.5
    /// setup은 사용자가 권한 창에 답할 때까지 기다린다.
    static let setup: TimeInterval = 120
    /// status는 읽기만 한다.
    static let status: TimeInterval = 4
    /// 클릭으로 뜬 뒤 응답이 오지 않으면 끝낸다.
    static let idle: TimeInterval = 15
    /// kairos reveal이 Orca를 앞으로 가져올 때까지 기다린다.
    static let reveal: TimeInterval = 10
}

let categoryID = "kairos.alert"
let openActionID = "kairos.open"
let logger = Logger(subsystem: "dev.kairos.notifier", category: "notifier")

func printError(_ message: String) {
    FileHandle.standardError.write(Data((message + "\n").utf8))
}

/// 시간 안에 끝나지 않으면 프로세스를 끝낸다. 주 스레드가 멈춰도 돌도록 전역 큐에 건다.
func armWatchdog(after seconds: TimeInterval, code: Int32, message: String) {
    DispatchQueue.global(qos: .utility).asyncAfter(deadline: .now() + seconds) {
        printError(message)
        exit(code)
    }
}

// MARK: - 클릭

/// 클릭 때 부를 kairos다. 설치본은 <rt>/ops/notifier/KairosNotifier.app에 놓이므로 <rt>/bin/kairos를 먼저 본다.
/// 없으면 ~/.local/bin/kairos를 쓴다. 두 경로 모두 알림 내용이나 defaults에서 받지 않는다.
func kairosExecutable() -> URL? {
    let runtime = Bundle.main.bundleURL
        .deletingLastPathComponent()
        .deletingLastPathComponent()
        .deletingLastPathComponent()
    let candidates = [
        runtime.appendingPathComponent("bin/kairos"),
        FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".local/bin/kairos"),
    ]
    return candidates.first { url in
        var isDirectory: ObjCBool = false
        let path = url.standardizedFileURL.path
        return FileManager.default.fileExists(atPath: path, isDirectory: &isDirectory)
            && !isDirectory.boolValue
            && FileManager.default.isExecutableFile(atPath: path)
    }
}

// MARK: - 앱

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate, UNUserNotificationCenterDelegate {
    private let mode: Mode
    private var responded = false
    /// 받았지만 아직 처리하지 않은 클릭 응답 수다. delegate 콜백은 주 스레드 밖에서 오므로 잠금으로 센다.
    private let pendingResponses = OSAllocatedUnfairLock(initialState: 0)
    /// 클릭 응답을 처리하느라 미룬 종료 코드다.
    private var deferredExit: Int32?

    init(mode: Mode) {
        self.mode = mode
    }

    private var center: UNUserNotificationCenter { .current() }

    // 클릭으로 다시 뜰 때 응답은 실행 직후에 온다. 그 전에 delegate가 없으면 응답을 잃는다.
    // post, setup, status 프로세스도 같은 번들이라 살아 있는 동안 클릭 응답을 받을 수 있다. 그래서 모든 모드에서 건다.
    func applicationWillFinishLaunching(_ notification: Notification) {
        center.delegate = self
        let open = UNNotificationAction(identifier: openActionID, title: "패널에서 보기", options: [.foreground])
        let category = UNNotificationCategory(
            identifier: categoryID, actions: [open], intentIdentifiers: [], options: []
        )
        center.setNotificationCategories([category])
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        switch mode {
        case .post(let payload):
            Task { finish(await post(payload)) }
        case .setup:
            Task { finish(await setup()) }
        case .status:
            Task { finish(await status()) }
        case .launch:
            Task {
                try? await Task.sleep(for: .seconds(Timeout.idle))
                if !responded { NSApp.terminate(nil) }
            }
        }
    }

    /// post, setup, status는 여기서 끝낸다. 그사이 받은 클릭이 있으면 kairos reveal을 띄운 뒤에 끝낸다.
    private func finish(_ code: Int32) {
        pendingResponses.withLock { count in
            // 잠근 채로 끝내야 그사이 들어온 클릭을 놓치지 않는다.
            if count == 0 { exit(code) }
        }
        deferredExit = code
    }

    // MARK: post

    private func post(_ payload: PostPayload) async -> Int32 {
        let settings = await center.notificationSettings()
        switch settings.authorizationStatus {
        case .authorized, .provisional, .ephemeral:
            break
        default:
            // 백그라운드 틱에서 권한 창을 띄우지 않는다. kairos는 이 알림을 보내지 못한 것으로 둔다.
            printError("kairos-notifier: 알림 권한이 없습니다. 'KairosNotifier setup'을 한 번 실행하세요")
            return ExitCode.notAuthorized
        }
        do {
            try await add(payload, level: payload.tier == .p0 ? .timeSensitive : .active)
        } catch where payload.tier == .p0 {
            // 긴급 알림 자격이 없는 서명이면 거절될 수 있다. 보통 알림으로 한 번 더 시도한다.
            logger.error("timeSensitive 게시 실패, active로 다시 시도: \(error.localizedDescription, privacy: .public)")
            do {
                try await add(payload, level: .active)
            } catch {
                printError("kairos-notifier: 알림 게시 실패: \(error.localizedDescription)")
                return ExitCode.rejected
            }
        } catch {
            printError("kairos-notifier: 알림 게시 실패: \(error.localizedDescription)")
            return ExitCode.rejected
        }
        return ExitCode.posted
    }

    private func add(_ payload: PostPayload, level: UNNotificationInterruptionLevel) async throws {
        let content = UNMutableNotificationContent()
        content.title = payload.title
        content.body = payload.body
        if let subtitle = payload.subtitle, !subtitle.isEmpty { content.subtitle = subtitle }
        content.threadIdentifier = payload.thread
        content.categoryIdentifier = categoryID
        content.userInfo = [itemsKey: payload.items.map { NSNumber(value: $0) }]
        content.sound = .default
        content.interruptionLevel = level
        content.relevanceScore = payload.tier == .p0 ? 1.0 : 0.5
        // 같은 항목 묶음을 다시 알리면 앞의 알림을 바꿔 끼운다.
        let request = UNNotificationRequest(identifier: requestID(payload), content: content, trigger: nil)
        try await center.add(request)
    }

    private func requestID(_ payload: PostPayload) -> String {
        let prefix = "kairos.\(payload.tier.rawValue)."
        guard !payload.items.isEmpty else {
            return prefix + String(Int64(Date().timeIntervalSince1970 * 1000))
        }
        let id = prefix + payload.items.map(String.init).joined(separator: "-")
        return String(id.prefix(Limit.requestID))
    }

    // MARK: setup, status

    private func setup() async -> Int32 {
        if #available(macOS 14, *) {
            NSApp.activate()
        } else {
            NSApp.activate(ignoringOtherApps: true)
        }
        let granted: Bool
        do {
            granted = try await center.requestAuthorization(options: [.alert, .sound, .badge])
        } catch {
            logger.error("권한 요청 실패: \(error.localizedDescription, privacy: .public)")
            granted = false
        }
        guard granted else {
            print("알림이 꺼져 있습니다. 시스템 설정 > 알림 > kairos에서 '알림 허용'을 켜세요.")
            print("켜기 전까지 kairos는 Mac 알림을 보내지 않습니다. 패널과 상태 표시줄에서만 보입니다.")
            return ExitCode.notAuthorized
        }
        let content = UNMutableNotificationContent()
        content.title = "알림이 켜졌습니다"
        content.body = "kairos 알림을 누르면 Orca 패널에서 그 일을 바로 엽니다."
        content.threadIdentifier = "kairos.setup"
        content.sound = .default
        do {
            try await center.add(UNNotificationRequest(identifier: "kairos.setup", content: content, trigger: nil))
        } catch {
            printError("kairos-notifier: 안내 알림 게시 실패: \(error.localizedDescription)")
            return ExitCode.rejected
        }
        print("알림이 켜졌습니다.")
        return ExitCode.posted
    }

    private func status() async -> Int32 {
        let settings = await center.notificationSettings()
        let name =
            switch settings.authorizationStatus {
            case .authorized: "authorized"
            case .denied: "denied"
            case .notDetermined: "notDetermined"
            case .provisional: "provisional"
            case .ephemeral: "ephemeral"
            @unknown default: "unknown"
            }
        print(name)
        return 0
    }

    // MARK: 클릭

    /// kairos reveal을 셸 없이 띄운다. 띄웠으면 true다. 기다리지 않는다.
    private func startReveal(_ items: [Int64]) -> Bool {
        guard !items.isEmpty else { return false }
        guard let executable = kairosExecutable() else {
            logger.error("kairos 실행 파일을 찾지 못했습니다")
            return false
        }
        let process = Process()
        process.executableURL = executable
        process.arguments = ["reveal"] + items.map(String.init)
        // post 프로세스의 출력은 kairos가 끝까지 읽는다. 자식이 물려받으면 kairos가 자식이 끝날 때까지 기다린다.
        process.standardInput = FileHandle.nullDevice
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        process.terminationHandler = { finished in
            let code = finished.terminationStatus
            Task { @MainActor in self.revealExited(code) }
        }
        do {
            try process.run()
        } catch {
            logger.error("kairos reveal 실행 실패: \(error.localizedDescription, privacy: .public)")
            return false
        }
        return true
    }

    private func revealExited(_ code: Int32) {
        if code != 0 { logger.error("kairos reveal 종료 코드 \(code, privacy: .public)") }
        if case .launch = mode { NSApp.terminate(nil) }
    }

    private func handleResponse(actionID: String, items: [Int64]) {
        responded = true
        let opens = actionID == UNNotificationDefaultActionIdentifier || actionID == openActionID
        let started = opens && startReveal(items)
        pendingResponses.withLock { $0 -= 1 }
        guard case .launch = mode else {
            // 짧게 사는 모드다. 자식을 띄웠으니 미뤄 둔 종료를 마저 한다. 아직 일하는 중이면 그 일이 끝낸다.
            if let code = deferredExit { finish(code) }
            return
        }
        guard started else {
            NSApp.terminate(nil)
            return
        }
        // reveal이 멈춰도 오래 남지 않는다. 자식은 그대로 두고 끝낸다.
        Task {
            try? await Task.sleep(for: .seconds(Timeout.reveal))
            NSApp.terminate(nil)
        }
    }

    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        willPresent notification: UNNotification,
        withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void
    ) {
        completionHandler([.banner, .list, .sound])
    }

    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        didReceive response: UNNotificationResponse,
        withCompletionHandler completionHandler: @escaping () -> Void
    ) {
        let actionID = response.actionIdentifier
        let items = clickedItems(response.notification.request.content.userInfo)
        // 주 스레드로 넘기기 전에 센다. 그사이 post가 끝나려 하면 finish가 이 수를 보고 기다린다.
        pendingResponses.withLock { $0 += 1 }
        completionHandler()
        Task { @MainActor in self.handleResponse(actionID: actionID, items: items) }
    }
}

// MARK: - 시작

let mode: Mode
do {
    mode = try parseMode(CommandLine.arguments)
} catch {
    switch error {
    case .usage: printError("사용법: KairosNotifier post '<json>' | setup | status")
    case .tooLarge: printError("kairos-notifier: 입력이 너무 큽니다")
    case .decode(let detail): printError("kairos-notifier: JSON을 읽지 못했습니다: \(detail)")
    case .empty(let field): printError("kairos-notifier: \(field) 값이 비었습니다")
    }
    exit(ExitCode.badInput)
}

// 감시는 AppKit을 띄우기 전에 건다. 부하가 높으면 앱이 뜨는 데만 1초 넘게 걸린다.
switch mode {
case .post:
    armWatchdog(after: Timeout.post, code: ExitCode.addFailed, message: "kairos-notifier: 알림 센터가 응답하지 않습니다")
case .setup:
    armWatchdog(after: Timeout.setup, code: ExitCode.notAuthorized, message: "kairos-notifier: 권한 창에 응답이 없어 끝냅니다")
case .status:
    armWatchdog(after: Timeout.status, code: ExitCode.addFailed, message: "kairos-notifier: 알림 센터가 응답하지 않습니다")
case .launch:
    break
}

let app = NSApplication.shared
let delegate = AppDelegate(mode: mode)
app.delegate = delegate
app.setActivationPolicy(.accessory)
app.run()
