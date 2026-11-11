// KairosNotifier의 입력 검사다. 앱(main.swift)과 자체 시험(Tests/main.swift)이 함께 컴파일한다.
// AppKit과 알림 센터를 쓰지 않는다. 그래서 시험이 알림을 띄우지 않는다.
import Foundation

enum ExitCode {
    static let posted: Int32 = 0
    static let badInput: Int32 = 2
    static let notAuthorized: Int32 = 3
    /// 알림 센터가 제때 답하지 않았다. 요청이 이미 들어갔을 수 있다. kairos는 다음 틱에 다시 보낸다.
    static let addFailed: Int32 = 4
    /// 알림 센터가 요청을 거절했다. 게시하지 않았다.
    static let rejected: Int32 = 5
}

enum Limit {
    /// post 인자 크기 상한이다. kairos가 넘기는 JSON은 1KB 안팎이다.
    static let payloadBytes = 16 * 1024
    /// 클릭 때 넘기는 항목 수 상한이다. kairos reveal도 20개까지 받는다.
    static let items = 20
    /// 항목 ID 상한(미포함)이다. kairos의 ITEM_ID(^[1-9][0-9]{0,14}$)와 같은 범위다.
    static let itemID: Int64 = 1_000_000_000_000_000
    /// 알림 요청 ID 길이 상한이다.
    static let requestID = 160
    static let title = 120
    static let subtitle = 120
    static let body = 600
}

let itemsKey = "items"

enum Tier: String, Decodable {
    case p0 = "P0"
    case p1 = "P1"
}

/// kairos NativeNotifier::payload가 만드는 JSON이다. subtitle만 선택이다.
struct PostPayload: Decodable {
    let title: String
    let body: String
    let subtitle: String?
    let items: [Int64]
    let tier: Tier
    let thread: String
}

/// 실행 방식이다. 인자를 읽는 단계에서 정하고, 잘못된 인자는 알림 센터를 건드리기 전에 2로 끝낸다.
enum Mode {
    case post(PostPayload)
    case setup
    case status
    case launch
}

enum InputError: Error {
    case usage
    case tooLarge
    case decode(String)
    case empty(String)
}

func clipped(_ text: String, _ limit: Int) -> String {
    let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
    return trimmed.count > limit ? String(trimmed.prefix(limit - 1)) + "…" : trimmed
}

/// 양의 정수만 남긴다. 순서를 지키고 중복을 빼며 최대 20개다.
func validItems(_ raw: [Int64]) -> [Int64] {
    var seen = Set<Int64>()
    var kept: [Int64] = []
    for id in raw where id > 0 && id < Limit.itemID && seen.insert(id).inserted {
        kept.append(id)
        if kept.count == Limit.items { break }
    }
    return kept
}

func parsePayload(_ json: String) throws(InputError) -> PostPayload {
    guard json.utf8.count <= Limit.payloadBytes else { throw .tooLarge }
    let decoded: PostPayload
    do {
        decoded = try JSONDecoder().decode(PostPayload.self, from: Data(json.utf8))
    } catch {
        throw .decode(String(describing: error))
    }
    let title = clipped(decoded.title, Limit.title)
    let body = clipped(decoded.body, Limit.body)
    let thread = clipped(decoded.thread, Limit.requestID)
    if title.isEmpty { throw .empty("title") }
    if body.isEmpty { throw .empty("body") }
    if thread.isEmpty { throw .empty("thread") }
    return PostPayload(
        title: title,
        body: body,
        subtitle: decoded.subtitle.map { clipped($0, Limit.subtitle) },
        items: validItems(decoded.items),
        tier: decoded.tier,
        thread: thread
    )
}

func parseMode(_ arguments: [String]) throws(InputError) -> Mode {
    // 예전 macOS는 Finder 실행 때 -psn_ 인자를 붙인다. 모드로 보지 않는다.
    let args = arguments.dropFirst().filter { !$0.hasPrefix("-psn_") }
    guard let mode = args.first else { return .launch }
    switch (mode, args.count) {
    case ("post", 2): return .post(try parsePayload(args[args.startIndex + 1]))
    case ("setup", 1): return .setup
    case ("status", 1): return .status
    default: throw .usage
    }
}

/// 알림의 userInfo에서 항목 ID를 꺼낸다. 숫자가 아니거나 범위를 벗어난 값은 버린다. 참거짓과 실수도 버린다.
func clickedItems(_ userInfo: [AnyHashable: Any]) -> [Int64] {
    guard let raw = userInfo[itemsKey] as? [Any] else { return [] }
    let numbers = raw.compactMap { value -> Int64? in
        guard let number = value as? NSNumber,
              CFGetTypeID(number) == CFNumberGetTypeID(),
              !CFNumberIsFloatType(number)
        else { return nil }
        return number.int64Value
    }
    return validItems(numbers)
}
