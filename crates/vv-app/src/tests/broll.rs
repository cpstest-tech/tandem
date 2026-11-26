use super::*;

fn ids(n: u64) -> MediaId {
    let mut pool: vv_core::id_map::IdMap<MediaId, ()> = Default::default();
    let mut last = None;
    for _ in 0..=n {
        last = Some(pool.insert(()));
    }
    last.unwrap()
}

#[test]
fn spoken_words_pick_the_media_named_after_them() {
    let (beach, city, dog) = (ids(0), ids(1), ids(2));
    let media = vec![
        (beach, "Spiaggia_tramonto.MP4".to_string(), 10.0),
        (city, "citta-notte.mov".to_string(), 1.5),
        (dog, "cane.jpg".to_string(), 3.0),
    ];
    let words: Vec<(String, f64, f64)> = [
        ("Oggi", 0.0),
        ("andiamo", 0.4),
        ("in", 0.9),
        ("spiagge", 1.0),
        ("bellissime,", 1.5),
        ("poi", 3.5),
        ("in", 3.8),
        ("città", 4.0),
        ("con", 5.0),
        ("il", 5.2),
        ("cane", 5.4),
    ]
    .iter()
    .map(|(w, s)| (w.to_string(), *s, s + 0.3))
    .collect();
    let shots = plan(&words, &media);
    assert_eq!(shots.len(), 2, "{shots:?}");
    assert_eq!(shots[0], (beach, 1.0, 3.0));
    assert_eq!(shots[1].0, city);
    assert_eq!(shots[1].2, 1.5, "no longer than the media");
}

#[test]
fn short_words_and_unrelated_names_do_not_match() {
    assert!(!same_word("in", "in"));
    assert!(!same_word("spiaggia", "spada"));
    assert!(same_word("spiaggia", "spiagge"));
    assert!(same_word("citta", "citta"));
}
