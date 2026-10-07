# Custom code

## Purpose

Code sent around jobs (header, between passes, footer) and by buttons
(`machine.custom_code`), with expressions.

## Behaviour

Each non-empty line is processed in order:

- `[expression]` parts are replaced by their value with three decimals
  (`S[$30/2]` → `S500.000`); an expression that cannot be evaluated is left as
  written;
- a line that is `?`, `!`, `~`, `ctrl-x` or `0xNN` is sent at once as a real-time
  byte (not queued); `!` counts as a user hold;
- any other line is queued as a command (letters keep their case).

Expressions: numbers (`1e-3` too), `+ - * / ^`, parentheses, unary minus, `abs`,
`neg`, `sign`. Variables: `left right top bottom width height` (drawing range of
the loaded program, 0 without one), `jogstep` and `jogspeed` (the jog settings
`jog.step_mm`, `jog.speed_mm_min`), `WCO.X/Y/Z`, `MPos.X/Y/Z`, `WPos.X/Y/Z` (work
position, DIV-101), every numeric `$N` of the board settings (`$30` defaults to
1000).

Defaults: header `G90 (use absolute coordinates)`, footer
`G0 X0 Y0 Z0 (move back to origin)`, passes: four comment lines (sent as empty lines).

## Configuration

`job.header`, `job.passes`, `job.footer`; `jog.step_mm` and `jog.speed_mm_min`
for `[jogstep]` and `[jogspeed]`.

## Tests

`expr.rs` unit tests, `host_behaviour.rs::custom_header_and_footer_with_expressions_and_immediates`,
`work_position_variables_are_the_work_position`, `custom_code_sees_the_jog_step_and_speed`,
`loop_count_repeats_the_program_with_the_passes_code`; conformance `tests/protocol/test_jobs.py`.

## Divergences

DIV-101. LaserGRBL's evaluator (third-party) has quirks with unusual input; FreeKerf
uses a small parser with standard precedence.
