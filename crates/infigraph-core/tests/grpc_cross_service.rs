//! Comprehensive gRPC cross-repo dependency coverage (AIF3X-331 #21).
//!
//! Layers under test:
//!   1. Contract extraction (PRODUCER side) — `.proto` service+rpc → GrpcService
//!      contracts. Exercises find_parent_class's `service` handling +
//!      extract_grpc_contracts. THIS WORKS TODAY and is covered by live tests.
//!   2. Client detection (CONSUMER side) — currently BLOCKED. detect_grpc_clients
//!      graph-queries `s.name CONTAINS '{Svc}Stub'`, but a stub import/usage
//!      (`from x_pb2_grpc import FooStub` / `FooStub(chan)`) leaves ZERO
//!      queryable trace in the graph (external imports are discarded at index
//!      time — verified). Client detection must SOURCE-SCAN like the HTTP path
//!      (scan_source_for_urls); tracked as task #21c. The client/e2e tests below
//!      are `#[ignore]`d — they document the intended behavior and will pass
//!      once source-scanning lands.
//!
//! Remote (Neo4j shared graph) invariant is a further `#[ignore]` (needs live
//! containers, `cargo test -- --ignored`), same convention as
//! remote_cross_service.rs.
//!
//! NOTE (scope): the combined-graph Phase-3 promotion of gRPC CALLS_SERVICE →
//! real CALLS edges is a separate, unshipped layer (task #21b).

use std::collections::HashMap;
use std::path::Path;

use infigraph_core::extract::extract_file;
use infigraph_core::graph::{GraphBackend, KuzuBackend};
use infigraph_core::multi::grpc::{detect_grpc_clients, extract_grpc_contracts};
use infigraph_core::multi::{self, ContractKind, Group, Registry, RepoEntry};
use infigraph_languages::bundled_registry;

// ── helpers ────────────────────────────────────────────────────────────────

/// Index one or more source files (real extraction) into a fresh Kuzu backend.
fn backend_with(files: &[(&str, &[u8])]) -> (tempfile::TempDir, Box<dyn GraphBackend>) {
    let registry = bundled_registry().unwrap();
    let mut extractions = Vec::new();
    for (path, src) in files {
        let ext_dot = format!(".{}", path.rsplit('.').next().unwrap_or(""));
        let pack = registry
            .for_extension(&ext_dot)
            .unwrap_or_else(|| panic!("no language pack for {ext_dot}"));
        extractions.push(extract_file(path, src, pack).unwrap());
    }
    let dir = tempfile::TempDir::new().unwrap();
    let backend = KuzuBackend::open(&dir.path().join("graph")).unwrap();
    backend.upsert_files_bulk(&extractions, true).unwrap();
    (dir, Box::new(backend))
}

fn make_repo(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::TempDir::new().expect("tempdir");
    for (rel, content) in files {
        let p = dir.path().join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&p, content).unwrap();
    }
    dir
}

fn repo_entry(name: &str, path: &Path) -> RepoEntry {
    RepoEntry {
        name: name.to_string(),
        path: path.to_path_buf(),
        languages: vec![],
        symbol_count: 0,
        module_count: 0,
        last_indexed_commit: None,
    }
}

fn two_repo_group(producer: (&str, &Path), consumer: (&str, &Path), group: &str) -> Registry {
    let mut registry = Registry {
        repos: HashMap::new(),
        groups: HashMap::new(),
    };
    registry
        .repos
        .insert(producer.0.to_string(), repo_entry(producer.0, producer.1));
    registry
        .repos
        .insert(consumer.0.to_string(), repo_entry(consumer.0, consumer.1));
    registry.groups.insert(
        group.to_string(),
        Group {
            name: group.to_string(),
            org: String::new(),
            repos: vec![producer.0.to_string(), consumer.0.to_string()],
            contracts: vec![],
        },
    );
    registry
}

const SINGLE_RPC_PROTO: &str =
    "syntax = \"proto3\";\nservice UserService {\n  rpc GetUser (GetUserRequest) returns (User);\n}\n";

// ── Layer 1: contract extraction ─────────────────────────────────────────────

#[test]
fn test_contracts_single_rpc() {
    let (_d, backend) = backend_with(&[("user.proto", SINGLE_RPC_PROTO.as_bytes())]);
    let contracts = extract_grpc_contracts(backend.as_ref());
    assert_eq!(contracts.len(), 1, "one RPC → one contract: {contracts:?}");
    assert_eq!(contracts[0].kind, ContractKind::GrpcService);
    assert_eq!(contracts[0].path, "/UserService/GetUser");
    assert_eq!(contracts[0].method, "GRPC");
}

#[test]
fn test_contracts_multi_rpc() {
    let src = "syntax = \"proto3\";\nservice UserService {\n  rpc GetUser (Req) returns (User);\n  rpc ListUsers (Req) returns (Users);\n  rpc DeleteUser (Req) returns (Empty);\n}\n";
    let (_d, backend) = backend_with(&[("user.proto", src.as_bytes())]);
    let mut paths: Vec<String> = extract_grpc_contracts(backend.as_ref())
        .into_iter()
        .map(|c| c.path)
        .collect();
    paths.sort();
    assert_eq!(
        paths,
        vec![
            "/UserService/DeleteUser",
            "/UserService/GetUser",
            "/UserService/ListUsers",
        ],
        "one contract per RPC"
    );
}

#[test]
fn test_contracts_multiple_services_one_proto() {
    let src = "syntax = \"proto3\";\nservice UserService {\n  rpc GetUser (Req) returns (User);\n}\nservice OrderService {\n  rpc GetOrder (Req) returns (Order);\n}\n";
    let (_d, backend) = backend_with(&[("svc.proto", src.as_bytes())]);
    let mut paths: Vec<String> = extract_grpc_contracts(backend.as_ref())
        .into_iter()
        .map(|c| c.path)
        .collect();
    paths.sort();
    assert_eq!(
        paths,
        vec!["/OrderService/GetOrder", "/UserService/GetUser"],
        "RPCs grouped under their own service, not cross-linked"
    );
}

#[test]
fn test_contracts_none_without_proto() {
    // A plain Python file must yield no gRPC contracts.
    let (_d, backend) = backend_with(&[("app.py", b"def handler():\n    return 1\n")]);
    assert!(extract_grpc_contracts(backend.as_ref()).is_empty());
}

// ── Layer 2: client detection (stub name patterns) ───────────────────────────

fn contracts_for(service: &str) -> Vec<multi::Contract> {
    vec![multi::Contract {
        kind: ContractKind::GrpcService,
        service: service.to_string(),
        method: "GRPC".to_string(),
        path: format!("/{service}/Rpc"),
        symbol_id: format!("proto::{service}"),
        file: "svc.proto".to_string(),
    }]
}

#[test]
#[ignore] // blocked on #21c: client detection must source-scan (stub import leaves no graph trace)
fn test_client_python_pb2_grpc() {
    // Python generated stub module: user_service_pb2_grpc + UserServiceStub.
    let (_d, backend) = backend_with(&[(
        "client.py",
        b"from user_service_pb2_grpc import UserServiceStub\n\ndef call(chan):\n    return UserServiceStub(chan)\n",
    )]);
    let deps = detect_grpc_clients(backend.as_ref(), &contracts_for("UserService"));
    assert!(
        deps.iter().any(|d| d.target_service == "UserService"),
        "should detect Python pb2_grpc / Stub client: {deps:?}"
    );
}

#[test]
#[ignore] // blocked on #21c: client detection must source-scan (stub ref leaves no graph trace)
fn test_client_stub_suffix() {
    let (_d, backend) = backend_with(&[(
        "c.go",
        b"package main\ntype x struct { s UserServiceStub }\n",
    )]);
    let deps = detect_grpc_clients(backend.as_ref(), &contracts_for("UserService"));
    assert!(deps.iter().any(|d| d.target_service == "UserService"));
}

#[test]
#[ignore] // blocked on #21c: client detection must source-scan (stub ref leaves no graph trace)
fn test_client_client_suffix() {
    let (_d, backend) = backend_with(&[(
        "c.ts",
        b"export class Wrapper { c: UserServiceClient | null = null; }\n",
    )]);
    let deps = detect_grpc_clients(backend.as_ref(), &contracts_for("UserService"));
    assert!(deps.iter().any(|d| d.target_service == "UserService"));
}

#[test]
fn test_client_no_false_match_on_unrelated_name() {
    // A symbol that doesn't match any stub pattern must not link.
    let (_d, backend) = backend_with(&[(
        "c.py",
        b"class UserServiceHelper:\n    pass\n\ndef unrelated():\n    return 1\n",
    )]);
    let deps = detect_grpc_clients(backend.as_ref(), &contracts_for("PaymentService"));
    assert!(
        deps.is_empty(),
        "unrelated names must not match PaymentService stubs: {deps:?}"
    );
}

#[test]
fn test_client_detection_empty_contracts_is_noop() {
    let (_d, backend) = backend_with(&[("c.py", b"class UserServiceStub:\n    pass\n")]);
    assert!(detect_grpc_clients(backend.as_ref(), &[]).is_empty());
}

// ── Layer 3: end-to-end local group build ────────────────────────────────────

const PROTO_PRODUCER: &str =
    "syntax = \"proto3\";\nservice UserService {\n  rpc GetUser (GetUserRequest) returns (User);\n}\n";

const PY_CONSUMER: &str = r#"from user_service_pb2_grpc import UserServiceStub

class Gateway:
    def __init__(self, channel):
        self.stub = UserServiceStub(channel)

    def fetch(self, uid):
        return self.stub.GetUser(uid)
"#;

/// PRODUCER side (works today): a .proto producer's service becomes a
/// GrpcService contract in the group after index → sync_group_contracts.
#[test]
fn test_local_grpc_producer_contract_lands_in_group() {
    let producer = make_repo(&[("user.proto", PROTO_PRODUCER)]);
    let consumer = make_repo(&[("gateway.py", PY_CONSUMER)]);
    let mut registry = two_repo_group(
        ("user-service", producer.path()),
        ("api-gateway", consumer.path()),
        "grpc-contract-group",
    );

    multi::index_group(&mut registry, "grpc-contract-group", true, bundled_registry)
        .expect("index_group");
    let contract_count =
        multi::sync_group_contracts(&mut registry, "grpc-contract-group", bundled_registry)
            .expect("sync_group_contracts");
    assert!(
        contract_count > 0,
        "expected a GrpcService contract from the .proto producer"
    );

    let group = registry.groups.get("grpc-contract-group").unwrap();
    assert!(
        group
            .contracts
            .iter()
            .any(|c| c.kind == ContractKind::GrpcService && c.path.starts_with("/UserService/")),
        "group should carry the UserService gRPC contract: {:?}",
        group.contracts
    );
}

/// FULL end-to-end (blocked on #21c): consumer references UserServiceStub, so
/// link_cross_service_calls should emit a cross-service edge into the producer.
/// Blocked because client detection needs source-scanning — the stub reference
/// leaves no graph trace, so the current graph-query client scan finds nothing.
#[test]
#[ignore] // blocked on #21c: client detection must source-scan
fn test_local_grpc_end_to_end_links_consumer_to_producer() {
    let producer = make_repo(&[("user.proto", PROTO_PRODUCER)]);
    let consumer = make_repo(&[("gateway.py", PY_CONSUMER)]);
    let mut registry = two_repo_group(
        ("user-service", producer.path()),
        ("api-gateway", consumer.path()),
        "grpc-test-group",
    );

    multi::index_group(&mut registry, "grpc-test-group", true, bundled_registry)
        .expect("index_group");
    multi::sync_group_contracts(&mut registry, "grpc-test-group", bundled_registry)
        .expect("sync_group_contracts");

    let linked = multi::link_cross_service_calls(&registry, "grpc-test-group", bundled_registry)
        .expect("link_cross_service_calls");
    assert!(
        linked > 0,
        "expected a cross-service edge from api-gateway (UserServiceStub) into \
         user-service's gRPC service"
    );
}

/// The producer repo must not be recorded as its own consumer: the .proto file
/// defines the service, but that is not a client dependency.
#[test]
fn test_local_grpc_producer_does_not_self_link() {
    let producer = make_repo(&[("user.proto", PROTO_PRODUCER)]);
    // Consumer here has NO stub reference — so the only gRPC symbols are in the
    // producer's own proto. No cross-service gRPC edge should be produced.
    let consumer = make_repo(&[("unrelated.py", "def noop():\n    return 0\n")]);
    let mut registry = two_repo_group(
        ("user-service", producer.path()),
        ("api-gateway", consumer.path()),
        "grpc-self-test-group",
    );

    multi::index_group(
        &mut registry,
        "grpc-self-test-group",
        true,
        bundled_registry,
    )
    .expect("index_group");
    multi::sync_group_contracts(&mut registry, "grpc-self-test-group", bundled_registry)
        .expect("sync_group_contracts");
    let deps =
        multi::detect_cross_service_deps(&registry, "grpc-self-test-group", bundled_registry)
            .expect("detect_cross_service_deps");
    assert!(
        !deps
            .iter()
            .any(|d| d.target_method == "GRPC" && d.caller_service == "user-service"),
        "producer must not self-link as a gRPC consumer: {deps:?}"
    );
}

// ── Remote (Neo4j shared graph) — requires live containers ───────────────────

/// Same producer/consumer invariant against a shared (Neo4j) graph. The gRPC
/// client scan in detect_cross_service_deps namespaces its Cypher to each repo's
/// org/repo prefix (like the HTTP scan), so a stub symbol in one repo must not
/// match another repo's namespace in the shared graph.
///
/// `#[ignore]`: needs live Neo4j + Postgres (see remote_cross_service.rs). Run
/// with `cargo test -- --ignored` against real containers.
#[test]
#[ignore]
fn test_remote_grpc_end_to_end_namespaced() {
    // Intentionally minimal: the remote harness (connect_neo4j/connect_pg,
    // index_group with a group.org set) lives in remote_cross_service.rs. This
    // stub documents the invariant and is a placeholder for the live-DB wiring
    // so the namespaced gRPC path has an explicit, named home. See task tracking
    // for the full remote fixture buildout.
    eprintln!(
        "SKIP unless run with --ignored against live Neo4j+Postgres; \
         verifies namespaced gRPC client scan in a shared graph"
    );
}
