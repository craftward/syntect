use syntect::parsing::{ParseState, Scope, ScopeStackOp, SyntaxDefinition, SyntaxSetBuilder};

#[test]
fn replay_can_retry_on_its_original_line_without_duplicating_the_prefix() {
    let syntax = SyntaxDefinition::load_from_str(
        r#"
name: Replay Retry
scope: source.replay
contexts:
  main:
    - match: 'prefix'
      scope: keyword.prefix
    - match: 'TRY'
      scope: keyword.control
      branch_point: choice
      branch: [deferred, immediate, fallback]
  deferred:
    - match: 'FAIL'
      fail: choice
    - match: '\w+'
      scope: string.unquoted
  immediate:
    - match: '\S'
      fail: choice
  fallback:
    - meta_scope: meta.fallback
    - match: '\w+'
      scope: constant.numeric
"#,
        true,
        None,
    )
    .unwrap();
    let mut builder = SyntaxSetBuilder::new();
    builder.add(syntax);
    let syntaxes = builder.build();
    let mut parser = ParseState::new(&syntaxes.syntaxes()[0]);
    parser.parse_line("prefix TRY value\n", &syntaxes).unwrap();
    let output = parser.parse_line("FAIL\n", &syntaxes).unwrap();
    use ScopeStackOp::{Pop, Push};
    let scope = |name| Scope::new(name).unwrap();
    assert_eq!(output.replay_ranges, vec![0..1]);
    assert_eq!(
        output.replayed,
        vec![vec![
            (0, Push(scope("source.replay"))),
            (0, Push(scope("keyword.prefix"))),
            (6, Pop(1)),
            (7, Push(scope("meta.fallback"))),
            (7, Push(scope("keyword.control"))),
            (10, Pop(1)),
            (11, Push(scope("constant.numeric"))),
            (16, Pop(1)),
        ]]
    );
}

fn parse_history(grammar: &str, lines: &[&str]) -> Vec<Vec<(usize, ScopeStackOp)>> {
    let mut builder = SyntaxSetBuilder::new();
    builder.add(SyntaxDefinition::load_from_str(grammar, true, None).unwrap());
    let syntaxes = builder.build();
    let mut parser = ParseState::new(&syntaxes.syntaxes()[0]);
    let mut history = Vec::new();
    for (current, line) in lines.iter().enumerate() {
        let output = parser.parse_line(line, &syntaxes).unwrap();
        let mut replayed = output.replayed.into_iter();
        for range in output.replay_ranges {
            assert!(range.end <= current);
            for index in range {
                history[index] = replayed.next().unwrap();
            }
        }
        assert!(replayed.next().is_none());
        history.push(output.ops);
        for (text, ops) in lines.iter().zip(&history) {
            assert!(ops.windows(2).all(|pair| pair[0].0 <= pair[1].0), "{ops:?}");
            assert!(ops.iter().all(|(offset, _)| text.is_char_boundary(*offset)));
        }
    }
    history
}

#[test]
fn nested_replay_can_fail_an_earlier_outer_branch() {
    let history = parse_history(
        r#"
name: Nested Replay
scope: source.replay
contexts:
  main:
    - match: 'prefix'
      scope: keyword.prefix
    - match: 'OUTER'
      branch_point: outer
      branch: [outer-try, fallback]
  outer-try:
    - match: 'INNER'
      branch_point: inner
      branch: [inner-try, inner-retry]
  inner-try:
    - match: 'FAIL_INNER'
      fail: inner
    - match: '\w+'
      scope: string.unquoted
  inner-retry:
    - match: '\S'
      fail: outer
  fallback:
    - match: '\w+'
      scope: constant.numeric
"#,
        &["prefix OUTER\n", "INNER value\n", "FAIL_INNER\n"],
    );
    use ScopeStackOp::{Pop, Push};
    assert_eq!(
        history[1],
        vec![
            (0, Push(Scope::new("constant.numeric").unwrap())),
            (5, Pop(1)),
            (6, Push(Scope::new("constant.numeric").unwrap())),
            (11, Pop(1)),
        ]
    );
    assert!(history[0].contains(&(0, Push(Scope::new("keyword.prefix").unwrap()))));
}

#[test]
fn branches_created_during_replay_keep_their_source_line_and_history() {
    let history = parse_history(
        r#"
name: Branch During Replay
scope: source.replay
contexts:
  main:
    - match: 'OUTER'
      branch_point: outer
      branch: [outer-try, outer-fallback]
  outer-try:
    - match: 'FAIL_OUTER'
      fail: outer
    - match: '\w+'
      scope: string.unquoted
  outer-fallback:
    - match: 'INNER'
      branch_point: inner
      branch: [inner-try, inner-fallback]
  inner-try:
    - match: 'FAIL_INNER'
      fail: inner
    - match: '\w+'
      scope: string.unquoted
  inner-fallback:
    - match: '\w+'
      scope: constant.numeric
"#,
        &["OUTER\n", "INNER value\n", "FAIL_OUTER\n", "FAIL_INNER\n"],
    );
    use ScopeStackOp::{Pop, Push};
    let numeric = Scope::new("constant.numeric").unwrap();
    assert_eq!(history[1], vec![(6, Push(numeric)), (11, Pop(1))]);
    assert_eq!(history[2], vec![(0, Push(numeric)), (10, Pop(1))]);
    assert_eq!(history[3], vec![(0, Push(numeric)), (10, Pop(1))]);
}

#[test]
fn exhausted_replay_discards_speculative_scopes_and_preserves_prefix() {
    let history = parse_history(
        r#"
name: Exhausted Replay
scope: source.replay
contexts:
  main:
    - match: 'prefix'
      scope: keyword.prefix
    - match: 'TRY'
      branch_point: choice
      branch: [deferred, immediate]
    - match: '\w+'
      scope: constant.numeric
  deferred:
    - meta_scope: meta.speculative
    - match: 'FAIL'
      fail: choice
  immediate:
    - meta_scope: meta.speculative
    - match: '\S'
      fail: choice
"#,
        &["prefix TRY value\n", "FAIL\n"],
    );
    use ScopeStackOp::{Pop, Push};
    assert_eq!(
        history[0],
        vec![
            (0, Push(Scope::new("source.replay").unwrap())),
            (0, Push(Scope::new("keyword.prefix").unwrap())),
            (6, Pop(1)),
            (11, Push(Scope::new("constant.numeric").unwrap())),
            (16, Pop(1)),
        ]
    );
}
