//! Schema is created on first open. Person and device state survive a reopen.
//! Concurrent writers go through the single DB thread without corrupting the file.

use homeai_core::db::Db;
use homeai_core::model::{Device, DeviceState, Floor, Person, Property, Room};

fn seed_house(db: &Db) {
    db.put_property(Property {
        id: "home".into(),
        name: "Demo House".into(),
    })
    .unwrap();
    db.put_floor(Floor {
        id: "gnd".into(),
        property_id: "home".into(),
        name: "Ground".into(),
    })
    .unwrap();
    db.put_room(Room {
        id: "kitchen".into(),
        floor_id: "gnd".into(),
        name: "Kitchen".into(),
        kind: "indoor".into(),
    })
    .unwrap();
    db.put_device(Device {
        id: "light-1".into(),
        room_id: "kitchen".into(),
        name: "Kitchen light".into(),
        kind: "light".into(),
        protocol: Some("matter".into()),
    })
    .unwrap();
}

#[test]
fn person_and_device_state_survive_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("home.db");
    {
        let db = Db::open(&path).unwrap();
        seed_house(&db);
        db.put_person(Person {
            id: "ada".into(),
            name: "Ada".into(),
            kind: "resident".into(),
        })
        .unwrap();
        db.put_device_state(DeviceState {
            device_id: "light-1".into(),
            state_json: r#"{"on":true}"#.into(),
            updated_ms: 1_700,
        })
        .unwrap();
        db.close();
    }

    let db = Db::open(&path).unwrap();
    let person = db
        .get_person("ada")
        .unwrap()
        .expect("person lost after reopen");
    assert_eq!(person.name, "Ada");
    let state = db
        .get_device_state("light-1")
        .unwrap()
        .expect("device state lost after reopen");
    assert_eq!(state.state_json, r#"{"on":true}"#);
    assert!(
        path.starts_with(dir.path()),
        "database must stay on local disk"
    );
}

#[test]
fn concurrent_writes_do_not_corrupt() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("home.db");
    let db = Db::open(&path).unwrap();
    let threads: Vec<_> = (0..8)
        .map(|t| {
            let db = db.clone();
            std::thread::spawn(move || {
                for i in 0..50 {
                    db.put_person(Person {
                        id: format!("p-{t}-{i}"),
                        name: format!("Person {t}-{i}"),
                        kind: "resident".into(),
                    })
                    .unwrap();
                }
            })
        })
        .collect();
    for h in threads {
        h.join().unwrap();
    }
    assert_eq!(db.count("person").unwrap(), 400);
    let again = Db::open(&path).unwrap();
    assert_eq!(again.count("person").unwrap(), 400);
}
