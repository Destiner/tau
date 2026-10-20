// Build with -D KEYCHAIN_TESTS -parse-as-library alongside signing-keychain.swift.
// These tests never call a keychain API, spawn processes, or consume real credentials.
import Foundation
import Security
import Darwin

final class FakeLifecycle {
    let path: String
    var search = ["/synthetic/login.keychain-db", "/synthetic/other.keychain-db"]
    var failDelete = false
    var failRestore = false
    var deletes = 0
    var restores = 0

    init(_ path: String) { self.path = path }

    func create() throws {
        try require(try readRecovery(path)?.phase == .creating, "Missing pre-mutation recovery")
        try Data("synthetic database".utf8).write(to: URL(fileURLWithPath: path + "/" + keychainName))
        try Data().write(to: URL(fileURLWithPath: path + "/.flD3051040"))
        search.append(path + "/" + keychainName)
    }

    var operations: LifecycleOperations {
        LifecycleOperations(searchPaths: { self.search }, deleteOwned: {
            self.deletes += 1
            try require(!self.failDelete, "Injected deletion failure")
            if fileInfo(self.path + "/" + keychainName) != nil {
                try require(unlink(self.path + "/" + keychainName) == 0, "Cannot delete fake database")
            }
        }, restore: { paths in
            self.restores += 1
            try require(!self.failRestore, "Injected restoration failure")
            self.search = paths
        })
    }
}

@main
struct SigningKeychainTests {
    static var checks = 0

    static func check(_ condition: @autoclosure () throws -> Bool) throws {
        try require(try condition(), "Pure test assertion failed")
        checks += 1
    }

    static func rejects(_ body: () throws -> Void) throws {
        do { try body() } catch { checks += 1; return }
        throw SigningFailure("Expected rejection")
    }

    static func input(_ value: String) throws -> Credentials {
        try Credentials.read(InputStream(data: Data(value.utf8)))
    }

    static func subject(_ teams: [String]) -> [String: Any] {
        [kSecOIDX509V1SubjectName as String: [kSecPropertyKeyValue as String:
            teams.map { [kSecPropertyKeyLabel as String: kSecOIDOrganizationalUnitName as String,
                         kSecPropertyKeyValue as String: $0] }]]
    }

    static func main() throws {
        let parsed = try input(#"{"certificate":"AAEC","password":"synthetic-not-a-secret"}"#)
        try check(parsed.password == "synthetic-not-a-secret")
        try check(try parsed.pkcs12() == Data([0, 1, 2]))
        try check(try input(#"{"certificate":"AA==","password":""}"#).password.isEmpty)
        for malformed in ["", "not-json", "{}", #"{"certificate":"!","password":"x"}"#,
                          #"{"certificate":"","password":"x"}"#,
                          #"{"certificate":"AA==","password":1}"#] {
            try rejects { _ = try input(malformed) }
        }
        try rejects { _ = try input(String(repeating: "x", count: Credentials.inputLimit + 1)) }
        // Exactly at the bound must fail JSON decoding, not the byte limit.
        do { _ = try input(String(repeating: " ", count: Credentials.inputLimit)) }
        catch let error as SigningFailure { try check(error.message == "Expected credential JSON on stdin") }
        do { _ = try input("invalid synthetic-sensitive-content") }
        catch let error as SigningFailure { try check(!error.message.contains("synthetic-sensitive-content")) }
        try check(try exactlyOne([42]) == 42)
        try rejects { _ = try exactlyOne([Int]()) }
        try rejects { _ = try exactlyOne([1, 2]) }
        try check(try certificateTeam(subject(["SYNTHETIC1"])) == "SYNTHETIC1")
        try rejects { _ = try certificateTeam(subject([])) }
        try rejects { _ = try certificateTeam(subject(["A", "B"])) }
        try rejects { _ = try certificateTeam(subject([""])) }
        try rejects { _ = try certificateTeam([:]) }

        let settings = signingKeychainSettings()
        try check(settings.version == SEC_KEYCHAIN_SETTINGS_VERS1)
        try check(!settings.lockOnSleep.boolValue && !settings.useLockInterval.boolValue)
        try check(settings.lockInterval == 2_147_483_647)

        let hex = try partitionDescription()
        try check(hex.count % 2 == 0 && hex.allSatisfy { $0.isHexDigit })
        let bytes = stride(from: 0, to: hex.count, by: 2).map { offset -> UInt8 in
            let start = hex.index(hex.startIndex, offsetBy: offset)
            return UInt8(hex[start..<hex.index(start, offsetBy: 2)], radix: 16)!
        }
        let plist = try PropertyListSerialization.propertyList(from: Data(bytes), format: nil) as? [String: [String]]
        try check(plist == ["Partitions": ["apple-tool:", "apple:", "codesign:"]])
        let output = try JSONEncoder().encode(PublicIdentity(keychain: "/synthetic/signing.keychain-db", identity: "ABC", name: "Synthetic", teamId: "TEAM"))
        let object = try JSONSerialization.jsonObject(with: output) as! [String: String]
        try check(Set(object.keys) == Set(["keychain", "identity", "name", "teamId"]))

        for unsafe in ["/", "/fresh", "relative/path", "/private/tmp/../fresh", "/private/tmp//fresh",
                       "/private/tmp/fresh/", "/private/tmp/a\0b", "/tmp/fresh", "/System/fresh",
                       "/Library/fresh", "/usr/local/fresh", "/private/etc/fresh", "/Applications/fresh",
                       "/private/tmp/Keychains/fresh", "/private/tmp/a.keychain-db"] {
            try rejects { try validatePath(unsafe) }
        }

        let root = "/private/tmp/tau-keychain-pure-" + UUID().uuidString
        try require(mkdir(root, 0o700) == 0, "Cannot create pure test root")
        defer { try? FileManager.default.removeItem(atPath: root) }
        let owned = root + "/owned"
        try validatePath(owned)
        try createOwned(owned)
        try verifyOwned(owned)
        try rejects { try createOwned(owned) }
        try check(fileInfo(owned)!.st_mode & 0o777 == 0o700)
        try check(fileInfo(owned + "/" + markerName)!.st_mode & 0o777 == 0o600)
        let alias = root + "/alias"
        try require(symlink(owned, alias) == 0, "Cannot create test symlink")
        try rejects { try verifyOwned(alias) }
        try rejects { try validatePath(alias + "/child") }
        try require(unlink(alias) == 0, "Cannot remove test symlink")
        let extra = owned + "/unexpected"
        try Data().write(to: URL(fileURLWithPath: extra))
        try rejects { try removeOwnedFiles(owned) }
        try check(fileInfo(owned + "/" + markerName) != nil)
        try require(unlink(extra) == 0, "Cannot remove test file")
        let link = owned + "/" + keychainName
        try require(symlink("/dev/null", link) == 0, "Cannot create test symlink")
        try rejects { try removeOwnedFiles(owned) }
        try require(unlink(link) == 0, "Cannot remove test symlink")
        let copy = root + "/copied"
        try require(mkdir(copy, 0o700) == 0, "Cannot create test directory")
        try FileManager.default.copyItem(atPath: owned + "/" + markerName, toPath: copy + "/" + markerName)
        try rejects { try verifyOwned(copy) }
        try Data("synthetic database placeholder".utf8).write(to: URL(fileURLWithPath: link))
        let companion = owned + "/.flD3051040"
        try Data().write(to: URL(fileURLWithPath: companion))
        try verifyOwned(owned)
        try check(fileInfo(companion)!.st_size == 0)
        try Data("not a lock".utf8).write(to: URL(fileURLWithPath: companion))
        try rejects { try removeOwnedFiles(owned) }
        try check(fileInfo(owned + "/" + markerName) != nil)
        try require(unlink(companion) == 0, "Cannot remove test file")
        try require(symlink("/dev/null", companion) == 0, "Cannot create test symlink")
        try rejects { try verifyOwned(owned) }
        try require(unlink(companion) == 0, "Cannot remove test symlink")
        let empty = root + "/empty"
        try Data().write(to: URL(fileURLWithPath: empty))
        try require(Darwin.link(empty, companion) == 0, "Cannot create test hardlink")
        try rejects { try verifyOwned(owned) }
        try require(unlink(companion) == 0, "Cannot remove test hardlink")
        let foreignLock = owned + "/.fl00000000"
        try Data().write(to: URL(fileURLWithPath: foreignLock))
        try rejects { try verifyOwned(owned) }
        try require(unlink(foreignLock) == 0, "Cannot remove test file")
        try Data().write(to: URL(fileURLWithPath: companion))
        try removeOwnedFiles(owned)
        try check(fileInfo(owned) == nil)
        try rejects { try removeOwnedFiles(owned) }
        // Failed framework deletion must retain the database, marker and original snapshot.
        let failedDelete = FakeLifecycle(root + "/failed-delete")
        let original = failedDelete.search
        failedDelete.failDelete = true
        try rejects {
            try runOwnedCreation(failedDelete.path, operations: failedDelete.operations) {
                try failedDelete.create()
                throw SigningFailure("Injected import/ACL failure")
            }
        }
        try check(fileInfo(failedDelete.path + "/" + keychainName) != nil)
        try check(fileInfo(failedDelete.path + "/" + markerName) != nil)
        try check(fileInfo(failedDelete.path + "/.flD3051040") != nil)
        try check(try readRecovery(failedDelete.path)?.searchPaths == original)
        try check(try readRecovery(failedDelete.path)?.phase == .creating)
        // A fresh adapter simulates a later helper process using only persisted state.
        let deleteRetry = FakeLifecycle(failedDelete.path)
        deleteRetry.search = ["/synthetic/temporary-global-state"]
        try cleanupOwned(deleteRetry.path, operations: deleteRetry.operations)
        try check(deleteRetry.search == original)
        try check(deleteRetry.deletes == 1)
        try check(fileInfo(deleteRetry.path) == nil)

        // Deletion succeeded but restoration did not: the absent DB is not completion.
        let failedRestore = FakeLifecycle(root + "/failed-restore")
        failedRestore.failRestore = true
        try rejects {
            try runOwnedCreation(failedRestore.path, operations: failedRestore.operations) {
                try failedRestore.create()
            }
        }
        try check(failedRestore.deletes == 1)
        try check(fileInfo(failedRestore.path + "/" + keychainName) == nil)
        try check(fileInfo(failedRestore.path + "/" + markerName) != nil)
        try check(try readRecovery(failedRestore.path)?.phase == .deleted)
        try check(try readRecovery(failedRestore.path)?.searchPaths == original)
        let restoreRetry = FakeLifecycle(failedRestore.path)
        restoreRetry.failDelete = true // Deleted state must never try to reopen/delete the absent DB.
        restoreRetry.failRestore = true
        try rejects { try cleanupOwned(restoreRetry.path, operations: restoreRetry.operations) }
        try check(fileInfo(restoreRetry.path + "/" + markerName) != nil)
        restoreRetry.failRestore = false
        try cleanupOwned(restoreRetry.path, operations: restoreRetry.operations)
        try check(restoreRetry.deletes == 0)
        try check(restoreRetry.search == original)
        try check(fileInfo(restoreRetry.path) == nil)

        // Normal removal captures current entries, not the old creation snapshot.
        let normal = FakeLifecycle(root + "/normal")
        try runOwnedCreation(normal.path, operations: normal.operations) { try normal.create() }
        try check(try readRecovery(normal.path)?.phase == .active)
        try check(try readRecovery(normal.path)?.searchPaths == [])
        normal.search = ["/synthetic/new-user-keychain", normal.path + "/" + keychainName, original[1]]
        normal.failDelete = true
        try rejects { try cleanupOwned(normal.path, operations: normal.operations) }
        try check(fileInfo(normal.path + "/" + keychainName) != nil)
        try check(try readRecovery(normal.path)?.phase == .deleting)
        let normalRetry = FakeLifecycle(normal.path)
        try cleanupOwned(normalRetry.path, operations: normalRetry.operations)
        try check(normalRetry.search == ["/synthetic/new-user-keychain", original[1]])
        try check(fileInfo(normalRetry.path) == nil)

        // Partial SecKeychainCreate failure can leave files without returning a handle.
        let partial = FakeLifecycle(root + "/partial")
        partial.failDelete = true
        partial.failRestore = true
        try rejects {
            try runOwnedCreation(partial.path, operations: partial.operations) {
                try partial.create()
                throw SigningFailure("Injected partial creation failure")
            }
        }
        try check(fileInfo(partial.path + "/" + keychainName) != nil)
        try check(fileInfo(partial.path + "/" + markerName) != nil)
        try check(try readRecovery(partial.path)?.searchPaths == original)
        let partialRetry = FakeLifecycle(partial.path)
        try cleanupOwned(partialRetry.path, operations: partialRetry.operations)
        try check(fileInfo(partialRetry.path) == nil)
        try check(partialRetry.search == original)

        // Successful rollback still removes a fresh directory on import failure.
        let rollback = FakeLifecycle(root + "/rollback")
        try rejects {
            try runOwnedCreation(rollback.path, operations: rollback.operations) {
                try rollback.create()
                throw SigningFailure("Injected wrong-password import failure")
            }
        }
        try check(rollback.search == original)
        try check(fileInfo(rollback.path) == nil)
        let early = FakeLifecycle(root + "/early")
        try rejects {
            try runOwnedCreation(early.path, operations: early.operations) {
                throw SigningFailure("Injected creation failure before database")
            }
        }
        try check(fileInfo(early.path) == nil)
        try check(early.search == original)
        print("Passed \(checks) pure checks; no Keychain APIs invoked")
    }
}
