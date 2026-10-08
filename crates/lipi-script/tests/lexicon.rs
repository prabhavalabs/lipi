//! The lexicon file format: build from text, save, load, and read a hand-written file.

use lipi_script::Lang;
use lipi_script::correct::{Corrector, Lexicon};

const TEXT: &str = "අමාත්‍ය මණ්ඩල තීරණය. අමාත්‍ය මණ්ඩලය 2024-05-15 දිනැති තීරණය; තීරණය (Cabinet) අනුමත කිරීම.";

#[test]
fn build_save_and_load() {
    let lexicon = Lexicon::from_text(Lang::Si, TEXT);
    assert_eq!(lexicon.tokens(), 10);
    assert_eq!(lexicon.freq("තීරණය"), 3);
    assert_eq!(lexicon.freq("අමාත්‍ය"), 2);
    assert_eq!(lexicon.freq("Cabinet"), 0);

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lexicon").join("si.lex");
    lexicon.save(&path).unwrap();

    let text = std::fs::read_to_string(&path).unwrap();
    let mut lines = text.lines();
    assert_eq!(lines.next(), Some("#lipi-lexicon 1 lang=si types=7 tokens=10"));
    assert_eq!(lines.next(), Some("තීරණය\t3"));
    assert_eq!(lines.next(), Some("අමාත්\u{200D}ය\t2"));
    let counts: Vec<u32> =
        text.lines().skip(1).map(|l| l.rsplit_once('\t').unwrap().1.parse().unwrap()).collect();
    assert!(counts.windows(2).all(|w| w[0] >= w[1]), "sorted by descending count");

    let loaded = Lexicon::load(&path).unwrap();
    assert_eq!(loaded, lexicon);
    assert_eq!(loaded.lang(), Lang::Si);
    assert_eq!(Corrector::new(loaded).lexicon().len(), 7);
}

#[test]
fn reads_hand_written_files_and_rejects_others() {
    let file = "#lipi-lexicon 1 lang=ta tokens=12\n# a comment\nஇலங்கை\t7\n\nஅரசு\t5\n";
    let l = Lexicon::read_from(file.as_bytes()).unwrap();
    assert_eq!((l.lang(), l.len(), l.tokens(), l.freq("இலங்கை")), (Lang::Ta, 2, 12, 7));

    assert!(Lexicon::read_from("".as_bytes()).is_err());
    assert!(Lexicon::read_from("word\t1\n".as_bytes()).is_err());
    assert!(Lexicon::read_from("#lipi-lexicon 1 lang=xx\n".as_bytes()).is_err());
    assert!(Lexicon::read_from("#lipi-lexicon 1 lang=si\nword 1\n".as_bytes()).is_err());
    assert!(Lexicon::read_from("#lipi-lexicon 1 lang=si\nword\tmany\n".as_bytes()).is_err());
}
