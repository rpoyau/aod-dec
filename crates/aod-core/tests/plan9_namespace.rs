use std::collections::BTreeMap;
use std::sync::Arc;

use aod_core::ns::{
    BindMode, CleanPath, ContentHash, MountMode, MountPerm, Namespace, NsError, Provider,
    ProviderVTable, UnitId, compose_subpath,
};

struct MapProvider {
    units: BTreeMap<String, String>,
    corrupted: bool,
}

impl ProviderVTable<String> for MapProvider {
    fn get_unit(&self, subpath: &[u8]) -> Result<Option<String>, NsError> {
        if self.corrupted {
            return Err(NsError::ProviderFault {
                path: String::from_utf8_lossy(subpath).to_string(),
                detail: "simulated digest corruption".to_string(),
            });
        }
        let key = CleanPath::clean_subpath(subpath)?;
        Ok(self.units.get(&key).cloned())
    }
}

fn make_provider(
    tag: &[u8],
    mode: MountMode,
    entries: &[(&str, &str)],
) -> Provider<Arc<dyn ProviderVTable<String>>> {
    let mut units = BTreeMap::new();
    for (k, v) in entries {
        units.insert((*k).to_string(), (*v).to_string());
    }
    let id = ContentHash::digest_domain(b"test.provider", &[tag]);
    let backend: Arc<dyn ProviderVTable<String>> = Arc::new(MapProvider {
        units,
        corrupted: false,
    });
    Provider::new(id, mode, MountPerm::ReadOnly, backend)
}

#[test]
fn plan9_cleanname_lexical_dot_dot_root_clamping_and_hostile_rejections() {
    assert_eq!(CleanPath::parse(b"/").unwrap().as_str(), "/");
    assert_eq!(CleanPath::parse(b"////").unwrap().as_str(), "/");
    assert_eq!(CleanPath::parse(b"/./../..///.").unwrap().as_str(), "/");
    assert_eq!(
        CleanPath::parse(b"/calc//trig/./../add/").unwrap().as_str(),
        "/calc/add"
    );
    assert_eq!(
        CleanPath::parse(b"/calc/../../etc/passwd").unwrap().as_str(),
        "/etc/passwd"
    );

    assert_eq!(CleanPath::parse(b""), Err(NsError::EmptyPath));
    assert!(matches!(
        CleanPath::parse(b"calc/add"),
        Err(NsError::MissingLeadingSlash(_))
    ));
    assert_eq!(CleanPath::parse(b"/calc/\0evil"), Err(NsError::NulByteInPath));
    assert!(matches!(
        CleanPath::parse(b"/calc\\evil"),
        Err(NsError::BackslashForbidden(_))
    ));
    assert_eq!(
        CleanPath::parse(b"/calc/\nnewline"),
        Err(NsError::ControlCharInPath)
    );
    assert_eq!(CleanPath::parse(&[b'/', 0xff]), Err(NsError::InvalidUtf8Path));

    // Provider-relative subpath clamps internal `..` but rejects climbing above mount root!
    assert_eq!(CleanPath::clean_subpath(b"").unwrap(), "");
    assert_eq!(CleanPath::clean_subpath(b".").unwrap(), "");
    assert_eq!(CleanPath::clean_subpath(b"a/b/../c").unwrap(), "a/c");
    assert!(matches!(
        CleanPath::clean_subpath(b"../escape"),
        Err(NsError::SubpathEscape(_))
    ));
    assert!(matches!(
        CleanPath::clean_subpath(b"a/../../escape"),
        Err(NsError::SubpathEscape(_))
    ));
    assert!(matches!(
        CleanPath::clean_subpath(b"/abs"),
        Err(NsError::SubpathEscape(_))
    ));

    let calc = CleanPath::parse_str("/calc").unwrap();
    let calc2 = CleanPath::parse_str("/calc2").unwrap();
    let calc_add = CleanPath::parse_str("/calc/add").unwrap();
    assert_eq!(calc.strip_mount_prefix(&calc), Some(""));
    assert_eq!(calc_add.strip_mount_prefix(&calc), Some("add"));
    assert_eq!(calc2.strip_mount_prefix(&calc), None);
    assert_eq!(compose_subpath("v1", "trig/sin"), "v1/trig/sin");
}

#[test]
fn cow_ternary_tree_mount_union_before_after_replace_and_o1_fork_isolation() {
    let mut ns = Namespace::empty();
    assert!(ns.is_empty());

    let p_base = make_provider(
        b"calc-1.0.0",
        MountMode::Static,
        &[("", "base_root"), ("add", "base_add"), ("mul", "base_mul")],
    );
    let p_fast = make_provider(
        b"fast-patch",
        MountMode::Static,
        &[("add", "fast_add")],
    );
    let p_fallback = make_provider(
        b"slow-fallback",
        MountMode::Dynamic,
        &[("add", "fallback_add"), ("div", "fallback_div")],
    );

    let calc_path = CleanPath::parse_str("/calc").unwrap();
    let dev_fast = CleanPath::parse_str("/dev/fast").unwrap();
    let dev_fb = CleanPath::parse_str("/dev/fb").unwrap();

    ns.mount(p_base.clone(), &calc_path, BindMode::Replace);
    ns.mount(p_fast.clone(), &dev_fast, BindMode::Replace);
    ns.mount(p_fallback.clone(), &dev_fb, BindMode::Replace);

    // Fork a COW snapshot BEFORE applying union overlays (`rfork(RFNAMEG)`)
    let parent_snap = ns.fork(false);
    // Isolated fork (`rfork(RFCNAMEG)` / `clear`) sees nothing at all!
    let sandbox_isolated = ns.fork(true);
    assert!(sandbox_isolated.is_empty());
    assert!(matches!(
        sandbox_isolated.lookup_unit(&CleanPath::parse_str("/calc/add").unwrap()),
        Err(NsError::NotMounted(_))
    ));

    // Apply BEFORE and AFTER union binds in `ns`; `parent_snap` must remain untouched!
    ns.bind(&dev_fast, &calc_path, BindMode::Before).unwrap();
    ns.bind(&dev_fb, &calc_path, BindMode::After).unwrap();

    // Verify `parent_snap` still sees `base_add` and `None` for `div`
    let (u_snap, m_snap, v_snap) = parent_snap
        .lookup_unit(&CleanPath::parse_str("/calc/add").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(v_snap, "base_add");
    assert_eq!(m_snap, MountMode::Static);
    assert_eq!(u_snap, UnitId::for_provider_unit(p_base.id, "add"));
    assert_eq!(
        parent_snap
            .lookup_unit(&CleanPath::parse_str("/calc/div").unwrap())
            .unwrap(),
        None
    );

    // Verify union order in `ns`: `/calc/add` -> `fast_add` (`p_fast.id`),
    // `/calc/mul` -> `base_mul` (`p_base.id`), `/calc/div` -> `fallback_div` (`p_fallback.id`, Dynamic)!
    let (u_add, m_add, v_add) = ns
        .lookup_unit(&CleanPath::parse_str("/calc/add").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(v_add, "fast_add");
    assert_eq!(m_add, MountMode::Static);
    assert_eq!(u_add, UnitId::for_provider_unit(p_fast.id, "add"));

    let (u_mul, _, v_mul) = ns
        .lookup_unit(&CleanPath::parse_str("/calc/mul").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(v_mul, "base_mul");
    assert_eq!(u_mul, UnitId::for_provider_unit(p_base.id, "mul"));

    let (u_div, m_div, v_div) = ns
        .lookup_unit(&CleanPath::parse_str("/calc/div").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(v_div, "fallback_div");
    assert_eq!(m_div, MountMode::Dynamic);
    assert_eq!(u_div, UnitId::for_provider_unit(p_fallback.id, "div"));
}

#[test]
fn eager_bind_prevents_cycles_supports_subtrees_and_aborts_on_provider_fault() {
    let mut ns = Namespace::empty();
    let p_a = make_provider(b"pkg-a", MountMode::Static, &[("sub/x", "from_a_sub_x")]);
    let p_b = make_provider(b"pkg-b", MountMode::Static, &[("y", "from_b_y")]);

    let path_a = CleanPath::parse_str("/a").unwrap();
    let path_a_sub = CleanPath::parse_str("/a/sub").unwrap();
    let path_b = CleanPath::parse_str("/b").unwrap();

    ns.mount(p_a.clone(), &path_a, BindMode::Replace);
    ns.mount(p_b.clone(), &path_b, BindMode::Replace);

    // Cross-binding `/a/sub -> /b` (Before) and `/b -> /a` (Before) cannot cycle!
    ns.bind(&path_a_sub, &path_b, BindMode::Before).unwrap();
    ns.bind(&path_b, &path_a, BindMode::Before).unwrap();

    let (uid_x, _, val_x) = ns
        .lookup_unit(&CleanPath::parse_str("/b/x").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(val_x, "from_a_sub_x");
    // Identity preserves original provider + composed subpath `"sub/x"`, NEVER string `"/b/x"`!
    assert_eq!(uid_x, UnitId::for_provider_unit(p_a.id, "sub/x"));

    let (_, _, val_y) = ns
        .lookup_unit(&CleanPath::parse_str("/a/y").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(val_y, "from_b_y");

    // Corrupted provider at front of union stack MUST abort immediately without silent fallthrough!
    let corrupted_backend: Arc<dyn ProviderVTable<String>> = Arc::new(MapProvider {
        units: BTreeMap::new(),
        corrupted: true,
    });
    let p_corrupt = Provider::new(
        ContentHash::digest_bytes(b"corrupt"),
        MountMode::Static,
        MountPerm::ReadOnly,
        corrupted_backend,
    );
    ns.mount(p_corrupt, &path_b, BindMode::Before);
    assert!(matches!(
        ns.lookup_unit(&CleanPath::parse_str("/b/y").unwrap()),
        Err(NsError::ProviderFault { .. })
    ));
}
