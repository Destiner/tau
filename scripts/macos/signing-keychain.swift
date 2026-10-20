// CI-only, disposable legacy keychain. No credential subprocesses or password files.
import Foundation
import Security
import CryptoKit
import Darwin

struct SigningFailure: Error {
    let message: String
    init(_ message: String) { self.message = message }
}

func require(_ condition: Bool, _ message: String) throws {
    if !condition { throw SigningFailure(message) }
}

func securityCheck(_ status: OSStatus, _ operation: String) throws {
    if status != errSecSuccess { throw SigningFailure("\(operation) failed (OSStatus \(status))") }
}

struct Credentials: Decodable {
    let certificate: String
    let password: String

    static let inputLimit = 2 * 1024 * 1024

    static func read(_ stream: InputStream) throws -> Credentials {
        stream.open()
        defer { stream.close() }
        var data = Data()
        var chunk = [UInt8](repeating: 0, count: 8192)
        while true {
            let count = stream.read(&chunk, maxLength: min(chunk.count, inputLimit + 1 - data.count))
            try require(count >= 0, "Cannot read credential input")
            if count == 0 { break }
            data.append(contentsOf: chunk.prefix(count))
            try require(data.count <= inputLimit, "Credential input exceeds limit")
        }
        // Never propagate decoder errors: they can contain fragments of the input.
        guard let value = try? JSONDecoder().decode(Self.self, from: data) else {
            throw SigningFailure("Expected credential JSON on stdin")
        }
        _ = try value.pkcs12()
        return value
    }

    func pkcs12() throws -> Data {
        guard let data = Data(base64Encoded: certificate), !data.isEmpty else {
            throw SigningFailure("Invalid base64 certificate")
        }
        return data
    }
}

let markerName = ".tau-signing-keychain.json"
let keychainName = "signing.keychain-db"
// Security's AtomicFile uses this exact zero-byte companion; never accept arbitrary .fl* files.
// apple-oss-distributions/Security: OSX/libsecurity_filedb/lib/AtomicFile.cpp
let keychainLockName = ".fl" + Insecure.SHA1.hash(data: Data(keychainName.utf8)).prefix(4)
    .map { String(format: "%02X", $0) }.joined()

struct Recovery: Codable {
    enum Phase: String, Codable { case creating, active, deleting, deleted }
    var phase: Phase
    var searchPaths: [String]
}

struct Ownership: Codable {
    var recovery: Recovery? = nil
    let format: String
    let path: String
    let uid: UInt32
    let device: Int32
    let inode: UInt64

    init(path: String, info: stat) {
        format = "tau-signing-keychain-v1"
        self.path = path
        uid = info.st_uid
        device = info.st_dev
        inode = info.st_ino
    }

    func matches(path: String, info: stat) -> Bool {
        format == "tau-signing-keychain-v1" && self.path == path
            && uid == geteuid() && uid == info.st_uid
            && device == info.st_dev && inode == info.st_ino
    }
}

func fileInfo(_ path: String) -> stat? {
    var value = stat()
    return lstat(path, &value) == 0 ? value : nil
}

func isDirectory(_ info: stat) -> Bool { info.st_mode & S_IFMT == S_IFDIR }
func isRegular(_ info: stat) -> Bool { info.st_mode & S_IFMT == S_IFREG }

// Reject aliases rather than resolving them (including /tmp -> /private/tmp).
// Same-UID processes and concurrent search-list writers are outside this CI helper's trust boundary.
func validatePath(_ path: String) throws {
    let parts = path.split(separator: "/", omittingEmptySubsequences: false)
    try require(path.hasPrefix("/") && path != "/" && !path.contains("\0"), "Unsafe directory")
    try require(parts.dropFirst().allSatisfy { !$0.isEmpty && $0 != "." && $0 != ".." }, "Unsafe directory")
    let lowered = path.lowercased()
    let protected = ["/system", "/library", "/usr", "/bin", "/sbin", "/etc", "/dev", "/volumes", "/applications", "/private/etc"]
    try require(!protected.contains { lowered == $0 || lowered.hasPrefix($0 + "/") }, "Protected directory")
    try require(!parts.contains { $0.lowercased() == "keychains" || $0.lowercased().contains(".keychain") || $0.lowercased() == "applications" }, "Protected directory")
    var ancestor = ""
    for part in parts.dropFirst().dropLast() {
        ancestor += "/" + part
        guard let info = fileInfo(ancestor), isDirectory(info) else { throw SigningFailure("Unsafe directory ancestor") }
        try require(info.st_mode & 0o022 == 0 || info.st_mode & S_ISVTX != 0, "Writable directory ancestor")
    }
    let parent = (path as NSString).deletingLastPathComponent
    try require(parent != "/", "Unsafe directory parent")
    guard let info = fileInfo(parent) else { throw SigningFailure("Missing directory parent") }
    try require(info.st_uid == geteuid() || info.st_mode & S_ISVTX != 0, "Directory parent is not owned")
}

func verifyOwned(_ path: String) throws {
    try validatePath(path)
    guard let info = fileInfo(path), isDirectory(info) else { throw SigningFailure("Missing owned directory") }
    try require(info.st_uid == geteuid() && info.st_mode & 0o777 == 0o700, "Unsafe owned directory")
    let marker = path + "/" + markerName
    guard let markerInfo = fileInfo(marker), isRegular(markerInfo), markerInfo.st_uid == geteuid(),
          markerInfo.st_nlink == 1, markerInfo.st_size <= 1024 * 1024, markerInfo.st_mode & 0o777 == 0o600,
          let data = try? Data(contentsOf: URL(fileURLWithPath: marker)),
          let owner = try? JSONDecoder().decode(Ownership.self, from: data), owner.matches(path: path, info: info)
    else { throw SigningFailure("Invalid ownership marker") }
    let allowed = Set([markerName, keychainName, keychainLockName, keychainName + "-wal", keychainName + "-shm", keychainName + "-journal"])
    guard let names = try? FileManager.default.contentsOfDirectory(atPath: path) else { throw SigningFailure("Cannot inspect owned directory") }
    for name in names {
        try require(allowed.contains(name), "Unexpected owned directory contents")
        guard let entry = fileInfo(path + "/" + name) else { throw SigningFailure("Cannot inspect owned file") }
        try require(isRegular(entry) && entry.st_uid == geteuid() && entry.st_nlink == 1, "Unsafe owned file")
        if name == keychainLockName { try require(entry.st_size == 0, "Invalid keychain lock file") }
    }
}

func createOwned(_ path: String) throws {
    try validatePath(path)
    try require(mkdir(path, 0o700) == 0, "Directory must be fresh and creatable")
    var marked = false
    defer { if !marked { _ = unlink(path + "/" + markerName); _ = rmdir(path) } }
    guard let info = fileInfo(path) else { throw SigningFailure("Cannot inspect new directory") }
    let data = try JSONEncoder().encode(Ownership(path: path, info: info))
    let fd = open(path + "/" + markerName, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW, 0o600)
    try require(fd >= 0, "Cannot create ownership marker")
    defer { close(fd) }
    let written = data.withUnsafeBytes { Darwin.write(fd, $0.baseAddress, $0.count) }
    try require(written == data.count, "Cannot write ownership marker")
    marked = true
}

func removeOwnedFiles(_ path: String) throws {
    try verifyOwned(path)
    guard let names = try? FileManager.default.contentsOfDirectory(atPath: path) else { throw SigningFailure("Cannot inspect owned directory") }
    for name in names where name != markerName {
        try require(unlink(path + "/" + name) == 0, "Cannot remove owned file")
    }
    try require(unlink(path + "/" + markerName) == 0, "Cannot remove ownership marker")
    try require(rmdir(path) == 0, "Cannot remove owned directory")
}

func readRecovery(_ path: String) throws -> Recovery? {
    try verifyOwned(path)
    return try JSONDecoder().decode(Ownership.self, from: Data(contentsOf: URL(fileURLWithPath: path + "/" + markerName))).recovery
}

func saveRecovery(_ path: String, _ recovery: Recovery) throws {
    try verifyOwned(path)
    let url = URL(fileURLWithPath: path + "/" + markerName)
    var owner = try JSONDecoder().decode(Ownership.self, from: Data(contentsOf: url))
    owner.recovery = recovery
    let data = try JSONEncoder().encode(owner)
    try require(data.count <= 1024 * 1024, "Recovery state exceeds limit")
    try data.write(to: url, options: .atomic)
    try require(chmod(url.path, 0o600) == 0, "Cannot protect recovery state")
}

// Only the Security operations are injected; ownership and durable state use real files.
struct LifecycleOperations {
    var searchPaths: () throws -> [String]
    var deleteOwned: () throws -> Void
    var restore: ([String]) throws -> Void
}

func cleanupOwned(_ path: String, operations: LifecycleOperations) throws {
    guard var recovery = try readRecovery(path) else {
        // A marker without recovery predates any Security mutation.
        try require(fileInfo(path + "/" + keychainName) == nil, "Missing keychain recovery state")
        try removeOwnedFiles(path)
        return
    }
    if recovery.phase == .active {
        // Never replay the creation snapshot over changes made during normal use.
        recovery.searchPaths = try operations.searchPaths().filter { $0 != path + "/" + keychainName }
        recovery.phase = .deleting
        try saveRecovery(path, recovery)
    }
    var deletionFailed = false
    if recovery.phase != .deleted {
        do {
            try operations.deleteOwned()
            recovery.phase = .deleted
            try saveRecovery(path, recovery)
        } catch { deletionFailed = true }
    }
    // Even a failed/partial creation may have changed the process-global search list.
    try operations.restore(recovery.searchPaths)
    try require(!deletionFailed, "Owned keychain deletion incomplete")
    try removeOwnedFiles(path)
}

func runOwnedCreation<T>(_ path: String, operations: LifecycleOperations, body: () throws -> T) throws -> T {
    let original = try operations.searchPaths()
    try createOwned(path)
    do {
        // Persist public paths before SecKeychainCreate can mutate global state.
        try saveRecovery(path, Recovery(phase: .creating, searchPaths: original))
        let result = try body()
        try verifyOwned(path)
        try operations.restore(original)
        try saveRecovery(path, Recovery(phase: .active, searchPaths: []))
        return result
    } catch {
        do { try cleanupOwned(path, operations: operations) }
        catch { throw SigningFailure("Creation failed; owned cleanup or search-list restoration incomplete") }
        throw error
    }
}

// Independently implemented from the API contract and observed SecurityTool algorithm:
// apple-oss-distributions/Security, SecurityTool/macOS/keychain_find.c, keychain_set_partition_list.
// No Apple source is copied (that implementation is APSL-2.0). Its wire format is
// a hex-encoded XML plist in the partition ACL description, not a comma-separated string.
func partitionDescription() throws -> String {
    let data = try PropertyListSerialization.data(fromPropertyList: ["Partitions": ["apple-tool:", "apple:", "codesign:"]], format: .xml, options: 0)
    return data.map { String(format: "%02x", $0) }.joined()
}

// Exported Security SPI, absent from public SDK headers. Signature verified against
// OSX/libsecurity_keychain/lib/SecKeychainItemPriv.h. Resolve before any creation;
// no fallback to prompting SecKeychainItemSetAccess or password-bearing argv.
typealias SetAccessWithPassword = @convention(c) (SecKeychainItem, SecAccess, UInt32, UnsafeRawPointer) -> OSStatus

func accessSetter() throws -> SetAccessWithPassword {
    guard let symbol = dlsym(UnsafeMutableRawPointer(bitPattern: -2), "SecKeychainItemSetAccessWithPassword") else {
        throw SigningFailure("Required Security access API unavailable")
    }
    return unsafeBitCast(symbol, to: SetAccessWithPassword.self)
}

func configurePartitions(_ key: SecKey, password: [UInt8], setter: SetAccessWithPassword) throws {
    let item = unsafeBitCast(key, to: SecKeychainItem.self)
    var access: SecAccess?
    try securityCheck(SecKeychainItemCopyAccess(item, &access), "Read key access")
    guard let access else { throw SigningFailure("Missing key access") }
    var list: CFArray?
    try securityCheck(SecAccessCopyACLList(access, &list), "Read key ACLs")
    guard let acls = list as? [SecACL] else { throw SigningFailure("Missing key ACLs") }
    var partitions = 0
    for acl in acls {
        let authorizations = SecACLCopyAuthorizations(acl) as? [String] ?? []
        if authorizations.contains(kSecACLAuthorizationPartitionID as String) {
            var apps: CFArray?
            var description: CFString?
            var selector = SecKeychainPromptSelector()
            try securityCheck(SecACLCopyContents(acl, &apps, &description, &selector), "Read partition ACL")
            try securityCheck(SecACLSetContents(acl, apps, try partitionDescription() as CFString, selector), "Configure partition ACL")
            partitions += 1
        }
    }
    // A modern file keychain must supply this ACL. Do not fabricate permissive ACLs.
    try require(partitions == 1, "Expected exactly one partition ACL")
    try password.withUnsafeBytes {
        try securityCheck(setter(item, access, UInt32($0.count), $0.baseAddress!), "Save key access")
    }
}

struct PublicIdentity: Encodable {
    let keychain: String
    let identity: String
    let name: String
    let teamId: String
}

func exactlyOne<T>(_ values: [T]) throws -> T {
    try require(values.count == 1, "Expected exactly one imported identity")
    return values[0]
}

func certificateTeam(_ values: [String: Any]) throws -> String {
    guard let subject = values[kSecOIDX509V1SubjectName as String] as? [String: Any],
          let entries = subject[kSecPropertyKeyValue as String] as? [[String: Any]] else {
        throw SigningFailure("Missing certificate subject")
    }
    let teams = entries.filter { ($0[kSecPropertyKeyLabel as String] as? String) == kSecOIDOrganizationalUnitName as String }
        .compactMap { $0[kSecPropertyKeyValue as String] as? String }
    let team = try exactlyOne(teams)
    try require(!team.isEmpty, "Empty certificate team OU")
    return team
}

func publicIdentity(_ identity: SecIdentity, path: String) throws -> PublicIdentity {
    var certificate: SecCertificate?
    try securityCheck(SecIdentityCopyCertificate(identity, &certificate), "Read imported certificate")
    guard let certificate, let name = SecCertificateCopySubjectSummary(certificate) as String? else {
        throw SigningFailure("Missing certificate subject")
    }
    let fingerprint = Insecure.SHA1.hash(data: SecCertificateCopyData(certificate) as Data)
        .map { String(format: "%02X", $0) }.joined()
    guard let values = SecCertificateCopyValues(certificate, [kSecOIDX509V1SubjectName] as CFArray, nil) as? [String: Any] else {
        throw SigningFailure("Missing certificate subject")
    }
    let team = try certificateTeam(values)
    return PublicIdentity(keychain: path, identity: fingerprint, name: name, teamId: team)
}

func copySearchList() throws -> CFArray {
    var list: CFArray?
    try securityCheck(SecKeychainCopySearchList(&list), "Capture search list")
    guard let list else { throw SigningFailure("Missing search list") }
    return list
}

func keychainPath(_ keychain: SecKeychain) throws -> String {
    var bytes = [CChar](repeating: 0, count: 4096)
    var size = UInt32(bytes.count)
    try securityCheck(SecKeychainGetPath(keychain, &size, &bytes), "Read keychain path")
    return String(cString: bytes)
}

func lifecycleOperations(_ path: String, created: @escaping () -> SecKeychain? = { nil }) -> LifecycleOperations {
    LifecycleOperations(searchPaths: {
        try (copySearchList() as! [SecKeychain]).map { try keychainPath($0) }
    }, deleteOwned: {
        var keychain = created()
        if keychain == nil && fileInfo(path + "/" + keychainName) != nil {
            try securityCheck(SecKeychainOpen(path + "/" + keychainName, &keychain), "Open owned keychain")
            try require(keychain != nil, "Missing owned keychain")
        }
        if let keychain { try securityCheck(SecKeychainDelete(keychain), "Delete owned keychain") }
    }, restore: { paths in
        let keychains: [SecKeychain] = try paths.map { path in
            var keychain: SecKeychain?
            try securityCheck(SecKeychainOpen(path, &keychain), "Open search-list keychain")
            guard let keychain else { throw SigningFailure("Missing search-list keychain") }
            return keychain
        }
        try securityCheck(SecKeychainSetSearchList(keychains as CFArray), "Restore search list")
    })
}

func createKeychain(_ path: String, credentials: Credentials) throws -> PublicIdentity {
    let setter = try accessSetter()
    var password = [UInt8](repeating: 0, count: 32)
    try securityCheck(SecRandomCopyBytes(kSecRandomDefault, password.count, &password), "Generate keychain password")
    defer { password.withUnsafeMutableBytes { _ = memset_s($0.baseAddress!, $0.count, 0, $0.count) } }
    var keychain: SecKeychain?
    return try runOwnedCreation(path, operations: lifecycleOperations(path, created: { keychain })) {
        try password.withUnsafeBytes {
            try securityCheck(SecKeychainCreate(path + "/" + keychainName, UInt32($0.count), $0.baseAddress!, false, nil, &keychain), "Create keychain")
        }
        guard let created = keychain else { throw SigningFailure("Missing created keychain") }
        var settings = SecKeychainSettings(version: UInt32(SEC_KEYCHAIN_SETTINGS_VERS1), lockOnSleep: false, useLockInterval: false, lockInterval: 0)
        try securityCheck(SecKeychainSetSettings(created, &settings), "Configure keychain")
        try password.withUnsafeBytes {
            try securityCheck(SecKeychainUnlock(created, UInt32($0.count), $0.baseAddress!, true), "Unlock keychain")
        }
        var codesign: SecTrustedApplication?
        try securityCheck(SecTrustedApplicationCreateFromPath("/usr/bin/codesign", &codesign), "Trust codesign")
        guard let codesign else { throw SigningFailure("Missing codesign application") }
        var access: SecAccess?
        try securityCheck(SecAccessCreate("Tau CI signing" as CFString, [codesign] as CFArray, &access), "Create import access")
        guard let access else { throw SigningFailure("Missing import access") }
        var imported: CFArray?
        let options = [kSecImportExportPassphrase: credentials.password as CFString,
                       kSecImportExportKeychain: created, kSecImportExportAccess: access] as CFDictionary
        try securityCheck(SecPKCS12Import(try credentials.pkcs12() as CFData, options, &imported), "Import PKCS12")
        guard let entries = imported as? [[String: Any]] else { throw SigningFailure("Missing import results") }
        let identities = entries.compactMap { $0[kSecImportItemIdentity as String] }.filter { CFGetTypeID($0 as CFTypeRef) == SecIdentityGetTypeID() }
        let identity = try exactlyOne(identities) as! SecIdentity
        var key: SecKey?
        try securityCheck(SecIdentityCopyPrivateKey(identity, &key), "Read imported key")
        guard let key else { throw SigningFailure("Missing imported key") }
        var actualKeychain: SecKeychain?
        try securityCheck(SecKeychainItemCopyKeychain(unsafeBitCast(key, to: SecKeychainItem.self), &actualKeychain), "Check imported key scope")
        try require(actualKeychain.map { CFEqual($0, created) } == true, "Imported key outside owned keychain")
        try configurePartitions(key, password: password, setter: setter)
        return try publicIdentity(identity, path: path + "/" + keychainName)
    }
}

func removeKeychain(_ path: String) throws {
    try cleanupOwned(path, operations: lifecycleOperations(path))
}

#if !KEYCHAIN_TESTS
@main
struct SigningKeychainCLI {
    static func main() {
        do {
            try require(CommandLine.arguments.count == 3, "Usage: signing-keychain create|remove <absolute fresh owned-directory>")
            try require(geteuid() != 0, "Do not run as root")
            let command = CommandLine.arguments[1]
            let path = CommandLine.arguments[2]
            try require(command == "create" || command == "remove", "Unknown command")
            try validatePath(path)
            // Process-local: no authorization dialogs, including on malformed imports.
            try securityCheck(SecKeychainSetUserInteractionAllowed(false), "Disable keychain interaction")
            if command == "create" {
                let credentials = try Credentials.read(InputStream(fileAtPath: "/dev/stdin")!)
                let metadata = try createKeychain(path, credentials: credentials)
                var output = try JSONEncoder().encode(metadata)
                output.append(0x0a)
                FileHandle.standardOutput.write(output)
            } else {
                try removeKeychain(path)
            }
        } catch {
            let message = (error as? SigningFailure)?.message ?? "Signing keychain operation failed"
            FileHandle.standardError.write(Data((message + "\n").utf8))
            exit(1)
        }
    }
}
#endif
