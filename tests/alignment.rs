use edlib_rs::aligner::{
    AlignConfig, Alignment, AlignmentMode, AlignmentTask, CigarFormat, EditOperation, EqualityPair,
    align,
};

const MODES: [AlignmentMode; 3] = [
    AlignmentMode::Global,
    AlignmentMode::Prefix,
    AlignmentMode::Infix,
];
const TASKS: [AlignmentTask; 3] = [
    AlignmentTask::Distance,
    AlignmentTask::Locations,
    AlignmentTask::Path,
];

/// Reference edit distance that does not use edlib's bit-vector implementation.
fn reference_distance(query: &[u8], target: &[u8], mode: AlignmentMode) -> u32 {
    let mut row: Vec<usize> = (0..=target.len())
        .map(|i| if mode == AlignmentMode::Infix { 0 } else { i })
        .collect();
    for (i, q) in query.iter().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, t) in target.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = (diagonal + usize::from(q != t))
                .min(row[j] + 1)
                .min(above + 1);
            diagonal = above;
        }
    }
    (if mode == AlignmentMode::Global {
        row[target.len()]
    } else {
        *row.iter().min().unwrap()
    }) as u32
}

/// Replays the path over the first span and checks it is a valid alignment
/// costing exactly the reported distance.
fn assert_path_is_valid(query: &[u8], target: &[u8], result: &Alignment) {
    let span = result.locations().unwrap().next().unwrap();
    let (mut q, mut t, mut cost) = (0, span.start, 0);
    for op in result.path().unwrap() {
        match op {
            EditOperation::Match => {
                assert_eq!(query[q], target[t]);
                q += 1;
                t += 1;
            }
            EditOperation::Mismatch => {
                assert_ne!(query[q], target[t]);
                q += 1;
                t += 1;
                cost += 1;
            }
            EditOperation::Insert => {
                q += 1;
                cost += 1;
            }
            EditOperation::Delete => {
                t += 1;
                cost += 1;
            }
        }
    }
    assert_eq!(
        (q, t, Some(cost)),
        (query.len(), span.end, result.edit_distance())
    );
}

#[test]
fn cigar_merges_standard_matches_and_preserves_indel_orientation() {
    let config = AlignConfig {
        task: AlignmentTask::Path,
        ..Default::default()
    };
    for (query, target, extended, standard) in [
        (b"ACGT".as_slice(), b"AGGT".as_slice(), "1=1X2=", "4M"),
        (b"ACGT", b"AGT", "1=1I2=", "1M1I2M"),
        (b"AGT", b"ACGT", "1=1D2=", "1M1D2M"),
    ] {
        let result = align(query, target, &config).unwrap();
        assert_eq!(result.edit_distance(), Some(1));
        assert_eq!(
            result.cigar(CigarFormat::Extended).unwrap().to_string(),
            extended
        );
        assert_eq!(
            result.cigar(CigarFormat::Standard).unwrap().to_string(),
            standard
        );
        assert_path_is_valid(query, target, &result);
    }
}

#[test]
fn infix_reports_all_locations_but_only_first_path() {
    let result = align(
        b"AC",
        b"TACGAC",
        &AlignConfig {
            mode: AlignmentMode::Infix,
            task: AlignmentTask::Path,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(result.edit_distance(), Some(0));
    assert_eq!(result.start_locations(), Some([1, 4].as_slice()));
    assert_eq!(result.end_locations(), &[2, 5]);
    assert_eq!(result.end_offsets().collect::<Vec<_>>(), [3, 6]);
    assert_eq!(
        result.locations().unwrap().collect::<Vec<_>>(),
        [1..3, 4..6]
    );
    assert_eq!(result.path(), Some([EditOperation::Match; 2].as_slice()));
}

#[test]
fn span_before_target_start_is_an_empty_range() {
    // Inserting the whole query before the target is one of the optimal
    // alignments; edlib reports it first, with inclusive end -1.
    for mode in [AlignmentMode::Prefix, AlignmentMode::Infix] {
        let config = AlignConfig {
            mode,
            task: AlignmentTask::Path,
            ..Default::default()
        };
        let result = align(b"CCTT", b"AAGG", &config).unwrap();
        assert_eq!(result.edit_distance(), Some(4));
        assert_eq!(result.end_locations()[0], -1);
        assert_eq!(result.end_offsets().next(), Some(0));
        assert_eq!(result.locations().unwrap().next(), Some(0..0));
        assert_eq!(
            result.cigar(CigarFormat::Extended).unwrap().to_string(),
            "4I"
        );
    }
}

#[test]
fn locations_require_start_locations() {
    let result = align(b"AC", b"AC", &AlignConfig::default()).unwrap();
    assert_eq!(result.end_offsets().collect::<Vec<_>>(), [2]);
    assert!(result.locations().is_none());
}

#[test]
fn bounded_search_distinguishes_no_match_from_zero_distance() {
    for task in TASKS {
        let mut config = AlignConfig {
            task,
            max_distance: Some(0),
            ..Default::default()
        };
        let miss = align(b"AC", b"AG", &config).unwrap();
        assert_eq!(miss.edit_distance(), None);
        assert!(miss.end_locations().is_empty());
        assert_eq!(miss.start_locations(), None);
        assert_eq!(miss.path(), None);
        assert!(miss.cigar(CigarFormat::Extended).is_none());

        let hit = align(b"AC", b"AC", &config).unwrap();
        assert_eq!(hit.edit_distance(), Some(0));
        assert_eq!(hit.end_locations(), &[1]);
        assert_eq!(
            hit.start_locations().is_some(),
            task != AlignmentTask::Distance
        );
        assert_eq!(hit.path().is_some(), task == AlignmentTask::Path);

        config.max_distance = Some(u32::MAX);
        assert_eq!(
            align(b"AC", b"AG", &config).unwrap().edit_distance(),
            Some(1)
        );
    }
}

#[test]
fn bytes_and_equalities_do_not_require_utf8_or_live_inputs() {
    let result = {
        let query = vec![0, 255, b'N'];
        let target = vec![0, 128, b'A'];
        let equalities = [EqualityPair::new(255, 128), EqualityPair::new(b'N', b'A')];
        let config = AlignConfig {
            task: AlignmentTask::Path,
            additional_equalities: &equalities,
            ..Default::default()
        };
        assert_eq!(
            align(&target, &query, &config).unwrap().edit_distance(),
            Some(0)
        );
        align(&query, &target, &config).unwrap()
    };
    assert_eq!(result.edit_distance(), Some(0));
    assert_eq!(result.path(), Some([EditOperation::Match; 3].as_slice()));
    assert_eq!(
        result.cigar(CigarFormat::Extended).unwrap().to_string(),
        "3="
    );

    let equalities = [EqualityPair::new(b'A', b'B'), EqualityPair::new(b'B', b'C')];
    let config = AlignConfig {
        additional_equalities: &equalities,
        ..Default::default()
    };
    assert_eq!(align(b"A", b"C", &config).unwrap().edit_distance(), Some(1));
}

#[test]
fn empty_inputs_honor_distance_limit_and_task() {
    let path = |config: AlignConfig, query: &[u8], target: &[u8]| {
        let result = align(query, target, &config).unwrap();
        result.cigar(CigarFormat::Extended).map(|c| c.to_string())
    };
    let config = AlignConfig {
        task: AlignmentTask::Path,
        ..Default::default()
    };
    assert_eq!(path(config, b"", b"AC").as_deref(), Some("2D"));
    assert_eq!(path(config, b"AC", b"").as_deref(), Some("2I"));
    assert_eq!(path(config, b"", b"").as_deref(), Some(""));
    for mode in [AlignmentMode::Prefix, AlignmentMode::Infix] {
        let config = AlignConfig { mode, ..config };
        assert_eq!(path(config, b"", b"AC").as_deref(), Some(""));
        assert_eq!(path(config, b"AC", b"").as_deref(), Some("2I"));
    }

    let bounded = AlignConfig {
        max_distance: Some(1),
        ..config
    };
    let miss = align(b"", b"AC", &bounded).unwrap();
    assert_eq!(miss.edit_distance(), None);
    assert!(miss.end_locations().is_empty());
    assert_eq!(miss.start_locations(), None);
    assert_eq!(miss.path(), None);

    let distance_only = align(b"", b"AC", &AlignConfig::default()).unwrap();
    assert_eq!(distance_only.end_locations(), &[1]);
    assert_eq!(distance_only.start_locations(), None);
    assert_eq!(distance_only.path(), None);
}

#[test]
fn alignment_modes_match_independent_edit_distance() {
    // Exhaust all binary strings up to length four, including the empty one.
    let sequences: Vec<Vec<u8>> = (0..=4)
        .flat_map(|len| {
            (0..1 << len)
                .map(move |bits| (0..len).map(|i| b'A' + ((bits >> i) & 1) as u8).collect())
        })
        .collect();
    for mode in MODES {
        for query in &sequences {
            for target in &sequences {
                let expected = reference_distance(query, target, mode);
                for task in TASKS {
                    for limit in [None, Some(0), Some(1)] {
                        let config = AlignConfig {
                            mode,
                            task,
                            max_distance: limit,
                            ..Default::default()
                        };
                        let context =
                            format!("{mode:?} {task:?} {limit:?}: {query:?} vs {target:?}");
                        let result = align(query, target, &config).unwrap();
                        let distance = match limit {
                            Some(k) if expected > k => None,
                            _ => Some(expected),
                        };
                        assert_eq!(result.edit_distance(), distance, "{context}");
                        assert_eq!(
                            result.end_locations().is_empty(),
                            distance.is_none(),
                            "{context}"
                        );
                        if distance.is_none() || task == AlignmentTask::Distance {
                            continue;
                        }
                        for span in result.locations().unwrap() {
                            // Every reported span must realize the optimal distance.
                            assert_eq!(
                                Some(reference_distance(
                                    query,
                                    &target[span.clone()],
                                    AlignmentMode::Global
                                )),
                                distance,
                                "{context} {span:?}"
                            );
                            if mode != AlignmentMode::Infix {
                                assert_eq!(span.start, 0, "{context}");
                            }
                            if mode == AlignmentMode::Global {
                                assert_eq!(span.end, target.len(), "{context}");
                            }
                        }
                        if task == AlignmentTask::Path {
                            assert_path_is_valid(query, target, &result);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn large_paths_are_valid() {
    // Large enough that edlib switches from traceback to Hirschberg.
    let mut state: u64 = 42;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let target: Vec<u8> = (0..20_000)
        .map(|_| b"ACGT"[(next() % 4) as usize])
        .collect();
    let mut query = Vec::new();
    for &base in &target[2_000..18_000] {
        match next() % 20 {
            0 => {}
            1 => query.extend([base, b'A']),
            2 => query.push(b"ACGT"[(next() % 4) as usize]),
            _ => query.push(base),
        }
    }
    for mode in MODES {
        let config = AlignConfig {
            mode,
            task: AlignmentTask::Path,
            ..Default::default()
        };
        assert_path_is_valid(&query, &target, &align(&query, &target, &config).unwrap());
    }
}

#[test]
fn results_move_between_threads() {
    let config = AlignConfig {
        task: AlignmentTask::Path,
        ..Default::default()
    };
    let result = std::thread::spawn(move || align(b"ACGT", b"AGGT", &config).unwrap())
        .join()
        .unwrap();
    std::thread::scope(|scope| {
        scope.spawn(|| assert_eq!(result.edit_distance(), Some(1)));
        scope.spawn(|| assert_eq!(result.path().map(<[_]>::len), Some(4)));
    });
}
