use conandatabase::{
    entities::{
        group::{ConnectionGroup, DBGroup},
        member::GroupMember,
        peer::{Peer, PeerData},
    },
    rusqlite::Connection,
};

/// peer.address must be a 62 char onion address (schema CHECK constraint)
fn addr(c: char) -> String {
    format!("{c}{}.onion", c.to_string().repeat(55))
}

#[test]
fn insert_member_registers_unknown_and_dedupes_known() {
    let path =
        std::env::temp_dir().join(format!("conan_member_test_{}.sqlite", std::process::id()));
    let _ = std::fs::remove_file(&path);
    conandatabase::setup_db(&path.to_string_lossy()).unwrap();
    let conn = Connection::open(&path).unwrap();

    // first peer becomes id 1 ("me"), required by insert_group
    let me = conn
        .insert_peer(Peer::build("me", &addr('a'), true))
        .unwrap();
    assert_eq!(me.id, 1);
    let grp = conn
        .insert_group(DBGroup::new(vec![1, 2, 3], "grp".into()))
        .unwrap();

    // unknown peer -> added as peer and as member
    let alice = conn
        .insert_member(grp.id, Peer::build("alice", &addr('b'), false), false)
        .unwrap();
    assert!(alice.id > 1);
    let members = conn.list_members(grp.id).unwrap();
    assert_eq!(
        members
            .iter()
            .filter(|(p, _)| p.address == addr('b'))
            .count(),
        1
    );

    // known peer (same address, fresh struct with id 0) ->
    // no new peer row, membership replaced instead of duplicated
    let again = conn
        .insert_member(grp.id, Peer::build("alice", &addr('b'), false), true)
        .unwrap();
    assert_eq!(again.id, alice.id);
    assert_eq!(conn.list_all_peers(false).unwrap().len(), 2); // me + alice
    let members = conn.list_members(grp.id).unwrap();
    let alice_rows: Vec<_> = members
        .iter()
        .filter(|(p, _)| p.address == addr('b'))
        .collect();
    assert_eq!(alice_rows.len(), 1);
    assert!(alice_rows[0].1); // is_admin replaced to true

    let _ = std::fs::remove_file(&path);
}
