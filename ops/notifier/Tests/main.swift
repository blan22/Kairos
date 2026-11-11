// KairosNotifier 입력 검사의 자체 시험이다. build.sh가 번들을 만들기 전에 Payload.swift와 함께 컴파일해 돌린다.
// 알림 센터를 쓰지 않는다. 실패가 하나라도 있으면 1로 끝난다.
import Foundation

var failures = 0
var checks = 0

@MainActor
func expect(_ condition: Bool, _ name: String) {
    checks += 1
    if !condition {
        failures += 1
        print("실패: \(name)")
    }
}

func mode(_ args: String...) -> Mode? {
    try? parseMode(["KairosNotifier"] + args)
}

func inputError(_ json: String) -> InputError? {
    do {
        _ = try parsePayload(json)
        return nil
    } catch {
        return error
    }
}

func isDecode(_ error: InputError?) -> Bool {
    if case .decode? = error { return true }
    return false
}

func isEmpty(_ error: InputError?, _ field: String) -> Bool {
    if case .empty(let name)? = error { return name == field }
    return false
}

func isTooLarge(_ error: InputError?) -> Bool {
    if case .tooLarge? = error { return true }
    return false
}

func isUsage(_ args: [String]) -> Bool {
    do {
        _ = try parseMode(["KairosNotifier"] + args)
        return false
    } catch {
        if case .usage = error { return true }
        return false
    }
}

func isLaunch(_ mode: Mode?) -> Bool {
    if case .launch? = mode { return true }
    return false
}

// kairos NativeNotifier::payload가 만드는 모양 그대로다. 키 이름을 바꾸면 양쪽을 함께 고친다.
let rustP0 = #"{"title":"긴급","body":"회의 임박 1건, 배포 당일 위험 1건","items":[12,34],"tier":"P0","thread":"kairos.p0"}"#
let rustP1 = #"{"title":"새로 온 일","body":"Slack 멘션 1건","items":[],"tier":"P1","thread":"kairos.p1"}"#

// MARK: validItems

expect(validItems([3, 0, -1, 3, 1_000_000_000_000_000, 999_999_999_999_999, 7]) == [3, 999_999_999_999_999, 7],
       "validItems는 0, 음수, 중복, 10^15 이상을 버리고 순서를 지킨다")
expect(validItems(Array(1...25)) == Array(1...20), "validItems는 20개까지만 남긴다")
expect(validItems([5, 5, 5] + Array(1...30)).count == 20, "중복을 뺀 뒤 20개를 센다")
expect(validItems([]) == [], "빈 목록은 비어 있다")
expect(Limit.itemID == 1_000_000_000_000_000, "ID 상한은 kairos ITEM_ID(최대 15자리)와 같다")

// MARK: clickedItems

expect(clickedItems([itemsKey: [NSNumber(value: 1), NSNumber(value: Int64(2))]]) == [1, 2], "정수 NSNumber를 꺼낸다")
expect(clickedItems([itemsKey: [NSNumber(value: 2.5), "3", NSNumber(value: true), NSNumber(value: 4)]]) == [4],
       "실수, 문자열, 참거짓은 버린다")
expect(clickedItems([itemsKey: (1...30).map { NSNumber(value: $0) }]).count == 20, "클릭 항목도 20개까지다")
expect(clickedItems([:]) == [], "items가 없으면 비어 있다")
expect(clickedItems([itemsKey: "1,2"]) == [], "배열이 아니면 비어 있다")
expect(clickedItems([itemsKey: [NSNumber(value: 1_000_000_000_000_000), NSNumber(value: -3)]]) == [],
       "범위를 벗어난 ID는 버린다")

// MARK: parsePayload

if let p0 = try? parsePayload(rustP0) {
    expect(p0.tier == .p0 && p0.thread == "kairos.p0" && p0.items == [12, 34] && p0.subtitle == nil,
           "kairos P0 payload를 읽는다")
} else {
    expect(false, "kairos P0 payload를 읽는다")
}
if let p1 = try? parsePayload(rustP1) {
    expect(p1.tier == .p1 && p1.items.isEmpty, "kairos P1 payload를 읽는다")
} else {
    expect(false, "kairos P1 payload를 읽는다")
}
let missingThread = #"{"title":"긴급","body":"b","items":[1],"tier":"P0"}"#
expect(isDecode(inputError(missingThread)), "thread가 없으면 decode 오류다")
let badTier = #"{"title":"t","body":"b","items":[1],"tier":"P2","thread":"x"}"#
expect(isDecode(inputError(badTier)), "tier가 P0, P1이 아니면 decode 오류다")
let stringItems = #"{"title":"t","body":"b","items":["1"],"tier":"P1","thread":"x"}"#
expect(isDecode(inputError(stringItems)), "문자열 ID는 decode 오류다")
expect(isDecode(inputError("not json")), "JSON이 아니면 decode 오류다")
let blankTitle = #"{"title":"  ","body":"b","items":[],"tier":"P1","thread":"x"}"#
expect(isEmpty(inputError(blankTitle), "title"), "빈 제목은 empty 오류다")
let blankThread = #"{"title":"t","body":"b","items":[],"tier":"P1","thread":"\n"}"#
expect(isEmpty(inputError(blankThread), "thread"), "빈 thread는 empty 오류다")
let huge = #"{"title":"t","body":""# + String(repeating: "가", count: Limit.payloadBytes) + #"","items":[],"tier":"P1","thread":"x"}"#
expect(isTooLarge(inputError(huge)), "16KB를 넘으면 tooLarge 오류다")
let long = #"{"title":""# + String(repeating: "a", count: 200) + #"","body":"b","items":[0,1,1],"tier":"P1","thread":"x"}"#
if let clippedPayload = try? parsePayload(long) {
    expect(clippedPayload.title.count == Limit.title && clippedPayload.title.hasSuffix("…"), "긴 제목은 120자로 자른다")
    expect(clippedPayload.items == [1], "payload의 ID도 거른다")
} else {
    expect(false, "긴 제목도 읽는다")
}

// MARK: parseMode

expect(isLaunch(mode()), "인자가 없으면 launch다")
expect(isLaunch(mode("-psn_0_12345")), "-psn_ 인자는 무시한다")
if case .setup? = mode("setup") { expect(true, "setup") } else { expect(false, "setup") }
if case .status? = mode("status") { expect(true, "status") } else { expect(false, "status") }
if case .post(let payload)? = mode("post", rustP0) {
    expect(payload.items == [12, 34], "post는 payload를 읽는다")
} else {
    expect(false, "post는 payload를 읽는다")
}
for args in [["post"], ["status", "x"], ["setup", "x"], ["bogus"], ["post", rustP0, "x"]] {
    expect(isUsage(args), "\(args)는 usage 오류다")
}

print("자체 시험 \(checks)개, 실패 \(failures)개")
exit(failures == 0 ? 0 : 1)
