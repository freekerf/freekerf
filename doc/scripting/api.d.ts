// FreeKerf script API. Generated from the action registry by `cargo xtask gen-docs`. Do not edit.
// Every method dispatches one action; the caller's tier is checked on every call.

interface AppAutomation {
  /**
   * The permissions the person gave scripts and agents for this session or always (revoked only by the person, in the app).
   *
   * Action `automation.grants`, tier `read`.
   */
  grants(params?: Record<string, unknown>): ({ caller: "user" | "macro" | "agent" | "test"; code_hash?: string | null; name: string; scope: "run" | "session" | "always"; tier: "safety" | "read" | "ui" | "edit" | "machine" | "laser" })[];
  /**
   * The actions scripts and agents ran or were refused, oldest first (the last 500; `after` skips the entries up to that number).
   *
   * Action `automation.log`, tier `read`.
   */
  log(params?: { after?: number | null }): ({ action: string; actor: { kind: "user" | "macro" | "agent" | "test"; name: string }; error?: string | null; seq: number })[];
  /**
   * Agents attached to the local endpoint, scripts running, permission requests waiting for the person.
   *
   * Action `automation.status`, tier `read`.
   */
  status(params?: Record<string, unknown>): { agents: string[]; requests: ({ action: string; actor: { kind: "user" | "macro" | "agent" | "test"; name: string }; id: number; tier: "safety" | "read" | "ui" | "edit" | "machine" | "laser" })[]; scripts: ({ actor: { kind: "user" | "macro" | "agent" | "test"; name: string }; id: number; name: string })[] };
}

interface AppConsole {
  /**
   * Console rows (sent commands, answers, messages).
   *
   * Action `console.get`, tier `read`.
   */
  get(params?: { last?: number | null }): ({ Command: { id: number; text: string } } | { Result: { decoded?: string | null; id: number; result: string; status: "Queued" | "WaitingResponse" | "ResponseGood" | "ResponseBad" | "InvalidResponse" } } | { Message: { decoded?: string | null; kind: "Startup" | "Config" | "Alarm" | "Feedback" | "Position" | "Other" | "Warning" | "Diagnostic"; text: string } })[];
}

interface AppDocument {
  /**
   * Adds a layer of a colour (line, 1000 mm/min, power 1000, 1 pass unless given). One undo step.
   *
   * Action `document.add_layer`, tier `edit`.
   */
  addLayer(params: { color: string; fill_pattern?: "horizontal" | "vertical" | "diagonal" | "reverse_diagonal" | "grid" | "diagonal_grid" | "cross" | "diagonal_cross" | "squares" | "zig_zag" | "hilbert" | "inset" | null; laser_mode?: "machine" | "constant" | "dynamic" | null; lines_per_mm?: number | null; mode?: "line" | "fill" | "fill_and_line" | "cut" | "ignore" | null; name?: string | null; passes?: number | null; power?: number | null; speed_mm_min?: number | null; visible?: boolean | null }): { id: number };
  /**
   * Draws a rectangle or an ellipse on a layer (default: the current layer) and selects it. One undo step.
   *
   * Action `document.add_shape`, tier `edit`.
   */
  addShape(params: { height: number; layer?: number | null; shape: "rect" | "ellipse"; width: number; x: number; y: number }): { id: number };
  /**
   * Writes text on a layer (default: the current layer) and selects it: a font of `fonts.list` (default Relief SingleLine, single-line), its style, alignment, spacing and effects. The text stays editable (`document.set_text`). One undo step.
   *
   * Action `document.add_text`, tier `edit`.
   */
  addText(params: { align?: "left" | "center" | "right" | "justify" | null; arc_radius_mm?: number | null; bold?: boolean | null; font?: string | null; height_mm?: number | null; italic?: boolean | null; layer?: number | null; letter_spacing_mm?: number | null; line_spacing?: number | null; outline_mm?: number | null; stencil_mm?: number | null; strikethrough?: boolean | null; text: string; underline?: boolean | null; weld?: boolean | null; width_scale?: number | null; x: number; y: number }): { id: number };
  /**
   * Lines up objects (default: the selection, at least two) on the box around them: left, center_h, right, top, middle_v or bottom. One undo step.
   *
   * Action `document.align`, tier `edit`.
   */
  align(params: { how: "left" | "center_h" | "right" | "top" | "middle_v" | "bottom"; ids?: number[] | null }): { changed: boolean };
  /**
   * Copies objects (default: the selection) in a grid of `columns` × `rows` (the originals are the top-left cell; rows go down), `gap_mm` apart; selects the originals and the copies. One undo step.
   *
   * Action `document.array`, tier `edit`.
   */
  array(params: { columns: number; gap_mm?: number[] | null; ids?: number[] | null; rows: number }): { ids: number[] };
  /**
   * Combines shapes (default: the selection, at least two) into one: union (weld), difference (the first in drawing order minus the others), intersection or xor. The result takes the first one's layer and is selected. One undo step.
   *
   * Action `document.boolean`, tier `edit`.
   */
  boolean(params: { ids?: number[] | null; op: "union" | "difference" | "intersection" | "xor" }): { id: number };
  /**
   * Selects nothing.
   *
   * Action `document.clear_selection`, tier `ui`.
   */
  clearSelection(params?: Record<string, unknown>): { changed: boolean };
  /**
   * Joins shapes (default: the selection) into one object per layer, so they move together; open paths whose ends touch (0.01 mm) become one continuous path, closed when it comes back to its start. Texts and images are left as they are. The results are selected. One undo step.
   *
   * Action `document.combine`, tier `edit`.
   */
  combine(params?: { ids?: number[] | null }): { ids: number[] };
  /**
   * Puts copies of objects (default: the selection) and their layers on the session's clipboard, which survives opening another document. Not an undo step.
   *
   * Action `document.copy`, tier `ui`.
   */
  copy(params?: { ids?: number[] | null }): { objects: number };
  /**
   * Puts objects (default: the selection) on the clipboard like `document.copy` and deletes them; the next paste puts them back in place. One undo step.
   *
   * Action `document.cut`, tier `edit`.
   */
  cut(params?: { ids?: number[] | null }): { objects: number };
  /**
   * Deletes objects (default: the selection). One undo step.
   *
   * Action `document.delete`, tier `edit`.
   */
  delete(params?: { ids?: number[] | null }): { changed: boolean };
  /**
   * Copies objects (default: the selection), offset by `offset_mm`, and selects the copies. One undo step.
   *
   * Action `document.duplicate`, tier `edit`.
   */
  duplicate(params?: { ids?: number[] | null; offset_mm?: number[] | null }): { ids: number[] };
  /**
   * Replaces the document with a `.fkproj` text (the undo history starts again).
   *
   * Action `document.from_fkproj`, tier `edit`.
   */
  fromFkproj(params: { text: string }): { changed: boolean };
  /**
   * The whole document (layers, objects with geometry and images), as in a `.fkproj` file.
   *
   * Action `document.get`, tier `read`.
   */
  get(params?: Record<string, unknown>): { format: string; layers: ({ color: string; fill?: { lines_per_mm: number; pattern: "horizontal" | "vertical" | "diagonal" | "reverse_diagonal" | "grid" | "diagonal_grid" | "cross" | "diagonal_cross" | "squares" | "zig_zag" | "hilbert" | "inset" }; id: number; laser_mode?: "machine" | "constant" | "dynamic"; mode?: "line" | "fill" | "fill_and_line" | "cut" | "ignore"; name?: string; passes?: number; power: number; speed_mm_min: number; visible?: boolean })[]; next_object_id?: number; objects: ({ kind: "shape"; paths: ({ closed?: boolean; from_current?: boolean; segments: unknown[]; start: { x: unknown; y: unknown } })[] } | { data: { bytes: string; kind: "encoded"; media_type: string } | { height: number; kind: "rgba"; pixels: string; width: number }; edits?: ({ op: unknown; rect: unknown; reference: unknown } | { op: unknown } | { argb: unknown; at: unknown; op: unknown; reference: unknown; tolerance: unknown } | { at: unknown; op: unknown; reference: unknown })[]; position: { x: number; y: number }; settings: { adjustments: { blue: number; brightness: number; contrast: number; formula: unknown; green: number; red: number; threshold?: number | null; white_clip: number }; border_speed_mm_min: number; centerline: { corner_threshold?: number | null; line_threshold?: number | null }; direction: "horizontal" | "vertical" | "diagonal"; dithering: "atkinson" | "floyd_steinberg" | "burks" | "jarvis" | "random" | "sierra2" | "sierra3" | "sierra_light" | "stucki"; interpolation: "nearest_neighbor" | "bilinear" | "bicubic" | "high_quality_bilinear" | "high_quality_bicubic"; laser_off: string; laser_on: string; lines_per_mm: number; mark_speed_mm_min: number; max_power: number; min_power: number; tool: "line_to_line" | "dithering" | "vectorize" | "centerline" | "passthrough"; vectorize: { adaptive_quality: boolean; down_sampling?: number | null; fill?: unknown; fill_lines_per_mm: number; optimize?: number | null; optimize_fast: boolean; smoothing?: number | null; spot_removal?: number | null } }; size_mm: number[] } | { element: string; kind: "unsupported"; reason: "not_converted" | "missing_comma" | "missing_argument" })[]; version: number };
  /**
   * Imports an SVG, DXF or image file into the document (format from the extension). One undo step.
   *
   * Action `document.import`, tier `edit`.
   */
  import(params: { path: string }): { objects: number[] };
  /**
   * Imports a file given as base64 bytes (`data`), or as `text` for SVG and DXF, into the document: SVG and DXF by colour layer (or all on the current layer), images on a layer of their own. Returns the new objects and layers. One undo step.
   *
   * Action `document.import_data`, tier `edit`.
   */
  importData(params: { colors?: "keep" | "unify"; data?: string | null; dpi?: number | null; format: "svg" | "dxf" | "image"; name?: string; power?: number | null; speed_mm_min?: number | null; text?: string | null }): { layers: number[]; objects: number[]; unsupported: number };
  /**
   * The layer of a colour, created (line, 1000 mm/min, power 1000) when missing; it becomes the current layer. Creating it is one undo step.
   *
   * Action `document.layer_for_color`, tier `edit`.
   */
  layerForColor(params: { color: string }): { created: boolean; layer: number };
  /**
   * Draws a material test (design/ux.md flow 6): a grid of cells, power rising by row and speed by column, each cell on a layer of its own with that power and speed, labelled in single-stroke text on a layer of low power. One undo step.
   *
   * Action `document.material_test`, tier `edit`.
   */
  materialTest(params: { cell_mm?: number | null; gap_mm?: number | null; max_s?: number | null; mode?: "fill" | "line"; origin?: number[] | null; power_pct: { from: number; steps: number; to: number }; speed_mm_min: { from: number; steps: number; to: number } }): { layers: number[]; objects: number[] };
  /**
   * Moves a layer in the layer list (the order of layers of the same mode is the order they run). One undo step.
   *
   * Action `document.move_layer`, tier `edit`.
   */
  moveLayer(params: { id: number; to: number }): { changed: boolean };
  /**
   * Moves objects (default: the selection) to another layer. One undo step.
   *
   * Action `document.move_to_layer`, tier `edit`.
   */
  moveToLayer(params: { ids?: number[] | null; layer: number }): { changed: boolean };
  /**
   * Starts an empty document (the undo history starts again).
   *
   * Action `document.new`, tier `edit`.
   */
  "new"(params?: Record<string, unknown>): { changed: boolean };
  /**
   * Adds the outline of shapes (default: the selection) grown or shrunk by `distance_mm`, as new objects on the same layers, selected; open paths (single-line text) are outlined around their strokes when growing. One undo step.
   *
   * Action `document.offset`, tier `edit`.
   */
  offset(params: { distance_mm: number; ids?: number[] | null; join?: "round" | "square" | "miter" }): { ids: number[] };
  /**
   * Opens a `.fkproj` project file (the undo history starts again).
   *
   * Action `document.open`, tier `edit`.
   */
  open(params: { path: string }): { path: string };
  /**
   * Adds the clipboard's objects (`document.copy`, `document.cut`) and selects them: each on the layer of its colour, created with the copied settings when missing. Offset by `offset_mm`, or centred on `at_mm`, by default `[5, -5]` mm once more for every paste in the document they were copied from. One undo step.
   *
   * Action `document.paste`, tier `edit`.
   */
  paste(params?: { at_mm?: number[] | null; offset_mm?: number[] | null }): { ids: number[] };
  /**
   * The bitmap an image object's raster settings (or the ones given) make: grey levels, or black and white when dithered; a PNG, base64.
   *
   * Action `document.raster_preview`, tier `read`.
   */
  rasterPreview(params: { id: number; settings?: { adjustments: { blue: number; brightness: number; contrast: number; formula: "simple_average" | "weight_average" | "optical_correct" | "custom"; green: number; red: number; threshold?: number | null; white_clip: number }; border_speed_mm_min: number; centerline: { corner_threshold?: number | null; line_threshold?: number | null }; direction: "horizontal" | "vertical" | "diagonal"; dithering: "atkinson" | "floyd_steinberg" | "burks" | "jarvis" | "random" | "sierra2" | "sierra3" | "sierra_light" | "stucki"; interpolation: "nearest_neighbor" | "bilinear" | "bicubic" | "high_quality_bilinear" | "high_quality_bicubic"; laser_off: string; laser_on: string; lines_per_mm: number; mark_speed_mm_min: number; max_power: number; min_power: number; tool: "line_to_line" | "dithering" | "vectorize" | "centerline" | "passthrough"; vectorize: { adaptive_quality: boolean; down_sampling?: number | null; fill?: { direction: unknown; kind: unknown } | null; fill_lines_per_mm: number; optimize?: number | null; optimize_fast: boolean; smoothing?: number | null; spot_removal?: number | null } } | null }): { height: number; png: string; width: number };
  /**
   * The raster dialog's settings of an image object (tool, dithering, resolution, grey adjustments, speeds, powers).
   *
   * Action `document.raster_settings`, tier `read`.
   */
  rasterSettings(params: { id: number }): { adjustments: { blue: number; brightness: number; contrast: number; formula: "simple_average" | "weight_average" | "optical_correct" | "custom"; green: number; red: number; threshold?: number | null; white_clip: number }; border_speed_mm_min: number; centerline: { corner_threshold?: number | null; line_threshold?: number | null }; direction: "horizontal" | "vertical" | "diagonal"; dithering: "atkinson" | "floyd_steinberg" | "burks" | "jarvis" | "random" | "sierra2" | "sierra3" | "sierra_light" | "stucki"; interpolation: "nearest_neighbor" | "bilinear" | "bicubic" | "high_quality_bilinear" | "high_quality_bicubic"; laser_off: string; laser_on: string; lines_per_mm: number; mark_speed_mm_min: number; max_power: number; min_power: number; tool: "line_to_line" | "dithering" | "vectorize" | "centerline" | "passthrough"; vectorize: { adaptive_quality: boolean; down_sampling?: number | null; fill?: { direction: "horizontal" | "vertical" | "diagonal"; kind: "raster" } | { direction: "horizontal" | "vertical" | "diagonal" | "reverse_diagonal" | "grid" | "diagonal_grid" | "cross" | "diagonal_cross" | "squares" | "zig_zag" | "hilbert" | "inset"; kind: "hatch" } | null; fill_lines_per_mm: number; optimize?: number | null; optimize_fast: boolean; smoothing?: number | null; spot_removal?: number | null } };
  /**
   * Removes a layer and its objects. One undo step.
   *
   * Action `document.remove_layer`, tier `edit`.
   */
  removeLayer(params: { id: number }): { changed: boolean };
  /**
   * Saves the document as a `.fkproj` project file (written atomically).
   *
   * Action `document.save`, tier `edit`.
   */
  save(params: { path: string }): { path: string };
  /**
   * Selects objects (replace, add to or remove from the selection). Unknown ids are ignored.
   *
   * Action `document.select`, tier `ui`.
   */
  select(params: { ids: number[]; mode?: "replace" | "add" | "remove" | "toggle" }): number[];
  /**
   * Selects every object.
   *
   * Action `document.select_all`, tier `ui`.
   */
  selectAll(params?: Record<string, unknown>): number;
  /**
   * Chooses the layer new shapes and text go to.
   *
   * Action `document.set_current_layer`, tier `ui`.
   */
  setCurrentLayer(params: { id: number }): { changed: boolean };
  /**
   * Changes the raster dialog's settings of an image object. One undo step.
   *
   * Action `document.set_raster`, tier `edit`.
   */
  setRaster(params: { id: number; settings: { adjustments: { blue: number; brightness: number; contrast: number; formula: "simple_average" | "weight_average" | "optical_correct" | "custom"; green: number; red: number; threshold?: number | null; white_clip: number }; border_speed_mm_min: number; centerline: { corner_threshold?: number | null; line_threshold?: number | null }; direction: "horizontal" | "vertical" | "diagonal"; dithering: "atkinson" | "floyd_steinberg" | "burks" | "jarvis" | "random" | "sierra2" | "sierra3" | "sierra_light" | "stucki"; interpolation: "nearest_neighbor" | "bilinear" | "bicubic" | "high_quality_bilinear" | "high_quality_bicubic"; laser_off: string; laser_on: string; lines_per_mm: number; mark_speed_mm_min: number; max_power: number; min_power: number; tool: "line_to_line" | "dithering" | "vectorize" | "centerline" | "passthrough"; vectorize: { adaptive_quality: boolean; down_sampling?: number | null; fill?: { direction: unknown; kind: "raster" } | { direction: unknown; kind: "hatch" } | null; fill_lines_per_mm: number; optimize?: number | null; optimize_fast: boolean; smoothing?: number | null; spot_removal?: number | null } } }): { changed: boolean };
  /**
   * Changes a text object: its text, height, font, style, alignment, spacing, effects, or where it sits on its path (its position, rotation and scale stay). One undo step.
   *
   * Action `document.set_text`, tier `edit`.
   */
  setText(params: { align?: "left" | "center" | "right" | "justify" | null; arc_radius_mm?: number | null; bold?: boolean | null; font?: string | null; height_mm?: number | null; id: number; italic?: boolean | null; letter_spacing_mm?: number | null; line_spacing?: number | null; outline_mm?: number | null; path_distance_mm?: number | null; path_flip?: boolean | null; path_offset_mm?: number | null; stencil_mm?: number | null; strikethrough?: boolean | null; text?: string | null; underline?: boolean | null; weld?: boolean | null; width_scale?: number | null }): { changed: boolean };
  /**
   * Splits shapes (default: the selection) made of several paths into one object per path, on the same layer; texts and images are left as they are. The pieces are selected. One undo step.
   *
   * Action `document.split`, tier `edit`.
   */
  split(params?: { ids?: number[] | null }): { ids: number[] };
  /**
   * Takes a text off its path: straight again, its baseline starting where the path starts. One undo step.
   *
   * Action `document.text_off_path`, tier `edit`.
   */
  textOffPath(params: { id: number }): { changed: boolean };
  /**
   * Puts a text on the first path of a shape: the letters stand along it, and follow it whenever the shape changes (moved, scaled, edited); moving the text alone puts it back on the path. Deleting the shape leaves the text where it was. One undo step.
   *
   * Action `document.text_on_path`, tier `edit`.
   */
  textOnPath(params: { distance_mm?: number | null; flip?: boolean | null; id: number; offset_mm?: number | null; path: number }): { changed: boolean };
  /**
   * Turns texts (default: the selected ones) into plain shapes: the letters stay, the text is no longer editable. One undo step.
   *
   * Action `document.text_to_paths`, tier `edit`.
   */
  textToPaths(params?: { ids?: number[] | null }): { changed: boolean };
  /**
   * The document as `.fkproj` text.
   *
   * Action `document.to_fkproj`, tier `read`.
   */
  toFkproj(params?: Record<string, unknown>): { text: string };
  /**
   * Moves, scales, mirrors or rotates objects (default: the selection): `{translate}`, `{scale, pivot}`, `{rotate_deg, pivot}` or `{matrix}`. Images only move and scale. One undo step.
   *
   * Action `document.transform`, tier `edit`.
   */
  transform(params: { ids?: number[] | null; op: { translate: number[] } | { pivot: number[]; scale: number[] } | { pivot: number[]; rotate_deg: number } | { matrix: number[] } }): { changed: boolean };
  /**
   * Moves objects (default: the selection) by `dx`, `dy` mm. One undo step.
   *
   * Action `document.translate`, tier `edit`.
   */
  translate(params: { dx: number; dy: number; ids?: number[] | null }): { changed: boolean };
  /**
   * Changes a layer's name, mode, speed, power, passes, visibility, filling or laser mode (M3/M4). One undo step.
   *
   * Action `document.update_layer`, tier `edit`.
   */
  updateLayer(params: { fill_pattern?: "horizontal" | "vertical" | "diagonal" | "reverse_diagonal" | "grid" | "diagonal_grid" | "cross" | "diagonal_cross" | "squares" | "zig_zag" | "hilbert" | "inset" | null; id: number; laser_mode?: "machine" | "constant" | "dynamic" | null; lines_per_mm?: number | null; mode?: "line" | "fill" | "fill_and_line" | "cut" | "ignore" | null; name?: string | null; passes?: number | null; power?: number | null; speed_mm_min?: number | null; visible?: boolean | null }): { changed: boolean };
}

interface AppFire {
  /**
   * Cancels the pending laser request (the one given, or any). Always allowed.
   *
   * Action `fire.cancel`, tier `safety`.
   */
  cancel(params?: { id?: number | null }): null;
  /**
   * Asks to run a laser action: it opens the fire confirmation (checklist and countdown) and runs only when a person confirms on screen. Returns the pending request.
   *
   * Action `fire.request`, tier `machine`.
   */
  request(params: { action: string; params?: unknown }): { action: string; countdown_ms: number; id: number; params: unknown; requested_by: string; summary: { bounds?: number[] | null; code?: string | null; estimated_ms?: number | null; lines?: number | null; program?: string | null } };
}

interface AppFonts {
  /**
   * The font families a text can use: the built-in single-line fonts first, then the installed ones.
   *
   * Action `fonts.list`, tier `read`.
   */
  list(params?: Record<string, unknown>): ({ bold: boolean; builtin: boolean; italic: boolean; name: string; single_line: boolean })[];
  /**
   * A sample of a font family as SVG path data (10 mm high, Y down), for font pickers.
   *
   * Action `fonts.preview`, tier `read`.
   */
  preview(params: { bold?: boolean; family: string; italic?: boolean; text?: string | null }): { d: string; single_line: boolean; view_box: number[] };
}

interface AppHistory {
  /**
   * Redoes the last undone change.
   *
   * Action `history.redo`, tier `edit`.
   */
  redo(params?: Record<string, unknown>): { step?: string | null };
  /**
   * Undoes the last document change.
   *
   * Action `history.undo`, tier `edit`.
   */
  undo(params?: Record<string, unknown>): { step?: string | null };
}

interface AppJob {
  /**
   * Stops streaming the job and sends `M5` (lines already in the board still run; use machine.stop to stop at once). Always allowed.
   *
   * Action `job.abort`, tier `safety`.
   */
  abort(params?: Record<string, unknown>): null;
  /**
   * Runs the head around the bounds of the loaded program with the laser off (jog moves).
   *
   * Action `job.frame`, tier `machine`.
   */
  frame(params?: { feed_mm_min?: number | null }): null;
  /**
   * Plans the document (vectors, then images; centerline uses the `autotrace` program) and loads it as the job, placed by `start` (absolute coordinates, the head's current position, or the user origin 0,0) and `anchor` (which point of the job's box goes there).
   *
   * Action `job.from_document`, tier `edit`.
   */
  fromDocument(params?: { anchor?: "bottom_left" | "bottom" | "bottom_right" | "left" | "center" | "right" | "top_left" | "top" | "top_right"; start?: "absolute" | "current_position" | "user_origin" }): { drawing_range?: number[] | null; estimated_ms: number; lines: number; name: string; quadrant: string };
  /**
   * The loaded program (name, lines, estimated time, range).
   *
   * Action `job.info`, tier `read`.
   */
  info(params?: Record<string, unknown>): { drawing_range?: number[] | null; estimated_ms: number; lines: number; name: string; quadrant: string } | null;
  /**
   * Loads and analyzes a G-code file.
   *
   * Action `job.load`, tier `edit`.
   */
  load(params: { path: string }): { drawing_range?: number[] | null; estimated_ms: number; lines: number; name: string; quadrant: string };
  /**
   * Loads and analyzes G-code given as text.
   *
   * Action `job.load_text`, tier `edit`.
   */
  loadText(params: { gcode: string; name: string }): { drawing_range?: number[] | null; estimated_ms: number; lines: number; name: string; quadrant: string };
  /**
   * Runs the loaded program from a position, rebuilding position, power and modes first. Fires the laser.
   *
   * Action `job.resume`, tier `laser`.
   */
  resume(params: { homing?: boolean; position: number; set_wco?: boolean }): null;
  /**
   * Runs the loaded program from the start (header, program, footer, passes). Fires the laser.
   *
   * Action `job.run`, tier `laser`.
   */
  run(params?: { homing?: boolean; passes?: number | null }): null;
}

interface AppMachine {
  /**
   * Connects to the machine on a serial port, or to the built-in emulator.
   *
   * Action `machine.connect`, tier `machine`.
   */
  connect(params?: { baud?: number | null; emulator?: boolean; port?: string | null }): null;
  /**
   * Runs custom code (buttons, macros): `[expr]` evaluated, immediate characters sent at once.
   *
   * Action `machine.custom_code`, tier `laser`.
   */
  customCode(params: { code: string }): null;
  /**
   * Closes the connection.
   *
   * Action `machine.disconnect`, tier `machine`.
   */
  disconnect(params?: Record<string, unknown>): null;
  /**
   * Feed hold (pause the motion). Always allowed.
   *
   * Action `machine.feed_hold`, tier `safety`.
   */
  feedHold(params?: Record<string, unknown>): null;
  /**
   * Runs the homing cycle (`$H`).
   *
   * Action `machine.home`, tier `machine`.
   */
  home(params?: Record<string, unknown>): null;
  /**
   * Work area, laser mode, highest power and laser module counters, from the board settings.
   *
   * Action `machine.info`, tier `read`.
   */
  info(params?: Record<string, unknown>): { homing_enabled: boolean; laser_in_use?: number | null; laser_life: ({ brand: string; classes_ms: number[]; death_date?: string | null; id: string; last_usage?: string | null; model: string; monitoring_date?: string | null; name: string; non_zero_ms: number; normalized_power_ms: number; optical_power_w?: number | null; purchase_date?: string | null; run_ms: number })[]; laser_mode: boolean; max_power: number; settings_known: boolean; table_height_mm: number; table_width_mm: number };
  /**
   * Jogs one step in a direction (laser off).
   *
   * Action `machine.jog`, tier `machine`.
   */
  jog(params: { direction: string; feed_mm_min?: number | null; step_mm?: number | null }): null;
  /**
   * Stops a continuous jog. Always allowed.
   *
   * Action `machine.jog_stop`, tier `safety`.
   */
  jogStop(params?: Record<string, unknown>): null;
  /**
   * Jogs to a work position (laser off).
   *
   * Action `machine.jog_to`, tier `machine`.
   */
  jogTo(params: { feed_mm_min?: number | null; x: number; y: number }): null;
  /**
   * Serial ports found on this computer.
   *
   * Action `machine.list_ports`, tier `read`.
   */
  listPorts(params?: Record<string, unknown>): ({ access: "ok" | "denied"; group?: string | null; path: string })[];
  /**
   * Sets the override targets; the host steps the board towards them.
   *
   * Action `machine.overrides`, tier `machine`.
   */
  overrides(params: { feed: number; power: number; rapid: number }): null;
  /**
   * Reads the board settings again (`$$`).
   *
   * Action `machine.read_settings`, tier `machine`.
   */
  readSettings(params?: Record<string, unknown>): null;
  /**
   * Resumes after a feed hold or a door (cycle start).
   *
   * Action `machine.resume`, tier `machine`.
   */
  resume(params?: Record<string, unknown>): null;
  /**
   * Safety door (real-time 0x84): the board holds as if its door opened; resume with machine.resume. Always allowed.
   *
   * Action `machine.safety_door`, tier `safety`.
   */
  safetyDoor(params?: Record<string, unknown>): null;
  /**
   * Selects the laser module in use (life counters).
   *
   * Action `machine.select_laser`, tier `edit`.
   */
  selectLaser(params: { id: string }): null;
  /**
   * Sends one line of G-code or a `$` command (not during a job). It can turn the laser on.
   *
   * Action `machine.send`, tier `laser`.
   */
  send(params: { line: string }): null;
  /**
   * Sets the current position as work zero (`G92 X0 Y0 Z0`).
   *
   * Action `machine.set_zero`, tier `machine`.
   */
  setZero(params?: Record<string, unknown>): null;
  /**
   * STOP: cancels running scripts, then soft resets the board (laser off, queues flushed). Always allowed.
   *
   * Action `machine.stop`, tier `safety`.
   */
  stop(params?: Record<string, unknown>): null;
  /**
   * Sends a `$` command (`$$`, `$I`, `$H`, `$X`, `$N=V`, `$J=...`; not during a job). It cannot turn the laser on.
   *
   * Action `machine.system_command`, tier `machine`.
   */
  systemCommand(params: { line: string }): null;
  /**
   * Unlocks the alarm state (`$X`).
   *
   * Action `machine.unlock`, tier `machine`.
   */
  unlock(params?: Record<string, unknown>): null;
  /**
   * Writes board settings (`$N=V`), one at a time; returns when the board answered all of them.
   *
   * Action `machine.write_settings`, tier `machine`.
   */
  writeSettings(params: { settings: Record<string, unknown> }): null;
}

interface AppMigrate {
  /**
   * What can be imported from LaserGRBL's data folder (`path`, default: LaserGRBL's folder on this computer): connection, preferences, laser counters, custom buttons, materials. Nothing is written.
   *
   * Action `migrate.detect`, tier `read`.
   */
  detect(params?: { path?: string | null }): { found: boolean; items: ({ count: number; kind: "connection" | "preferences" | "counters" | "buttons" | "materials" })[]; path?: string | null };
  /**
   * Imports the chosen items from LaserGRBL's data folder into FreeKerf's settings, machine cache and macro folder. LaserGRBL's files are only read.
   *
   * Action `migrate.run`, tier `edit`.
   */
  run(params: { items: ("connection" | "preferences" | "counters" | "buttons" | "materials")[]; path?: string | null }): { imported: ({ count: number; kind: "connection" | "preferences" | "counters" | "buttons" | "materials" })[]; warnings: string[] };
}

interface AppSession {
  /**
   * The session as front ends and agents read it: revision, selection, view, undo/redo labels, layers and objects without geometry.
   *
   * Action `session.snapshot`, tier `read`.
   */
  snapshot(params?: Record<string, unknown>): { clipboard: number; current_layer?: number | null; dirty: boolean; document: { bounds?: number[] | null; layers: ({ color: string; fill: { lines_per_mm: number; pattern: "horizontal" | "vertical" | "diagonal" | "reverse_diagonal" | "grid" | "diagonal_grid" | "cross" | "diagonal_cross" | "squares" | "zig_zag" | "hilbert" | "inset" }; id: number; laser_mode: "machine" | "constant" | "dynamic"; mode: "line" | "fill" | "fill_and_line" | "cut" | "ignore"; name: string; objects: number; passes: number; power: number; speed_mm_min: number; visible: boolean })[]; objects: ({ bounds?: number[] | null; id: number; image?: { dithering: unknown; lines_per_mm: number; tool: unknown } | null; kind: string; layer: number; name: string; text?: { align?: unknown; arc_radius_mm?: number | null; bold?: boolean; font?: string; height_mm: number; italic?: boolean; letter_spacing_mm?: number; line_spacing?: number; outline_mm?: number | null; path?: unknown; stencil_mm?: number | null; strikethrough?: boolean; text: string; underline?: boolean; weld?: boolean; width_scale?: number } | null; transform: number[] })[] }; document_revision: number; redo: string[]; revision: number; selection: number[]; undo: string[]; view: { dialogs: ("settings" | "shortcuts" | "fire_confirmation" | "recover" | "welcome" | "import" | "raster" | "material_test" | "about")[]; drawer_open: boolean; fire_request?: { action: string; countdown_ms: number; id: number; params: unknown; requested_by: string; summary: { bounds?: number[] | null; code?: string | null; estimated_ms?: number | null; lines?: number | null; program?: string | null } } | null; fit_request: number; focus?: string | null; mode: "design" | "produce"; panel: "layers" | "object" | "machine" | "console"; toasts: ({ args?: Record<string, unknown>; id: number; level: "info" | "ok" | "warning" | "error"; message: string })[]; tool: "select" | "rect" | "ellipse" | "text"; viewport?: { center: number[]; zoom: number } | null } };
}

interface AppSettings {
  /**
   * Reads the settings (all, or one dotted key).
   *
   * Action `settings.get`, tier `read`.
   */
  get(params?: { key?: string | null }): unknown;
  /**
   * Changes one setting and saves the file. Automation settings are user-only and refused here.
   *
   * Action `settings.set`, tier `edit`.
   */
  set(params: { key: string; value: unknown }): null;
}

interface AppState {
  /**
   * Machine state, loaded program, issues and resume offer.
   *
   * Action `state.get`, tier `read`.
   */
  get(params?: Record<string, unknown>): { firmware_version?: string | null; issues: ("Unknown" | "ManualReset" | "ManualDisconnect" | "ManualAbort" | "StopResponding" | "UnexpectedReset" | "UnexpectedDisconnect" | "MachineAlarm")[]; machine: { buffer_size: number; connected: boolean; detected_ip?: string | null; failed_connections: number; feed: number; firmware: "Grbl" | "Smoothie" | "Marlin" | "VigoWork"; grbl_blocks: number; grbl_buffer: number; in_program: boolean; job_errors: number; job_executed: number; job_sent: number; job_target: number; job_time_ms: number; jog_enabled: boolean; last_issue: "Unknown" | "ManualReset" | "ManualDisconnect" | "ManualAbort" | "StopResponding" | "UnexpectedReset" | "UnexpectedDisconnect" | "MachineAlarm"; loop_count: number; machine_position: { x: number; y: number; z: number }; override_targets: number[]; overrides: number[]; pending: number; power: number; program_estimated_ms: number; program_lines: number; projected_ms: number; queued: number; ready: boolean; status: "Disconnected" | "Connecting" | "Idle" | "Run" | "Hold" | "Door" | "Home" | "Alarm" | "Check" | "Jog" | "Queue" | "Cooling" | "AutoHold" | "Tool"; used_buffer: number; version?: { build?: string | null; hal: boolean; major: number; minor: number; vendor_info?: string | null; vendor_version?: string | null } | null; work_offset: { x: number; y: number; z: number }; work_position: { x: number; y: number; z: number } }; program?: { drawing_range?: number[] | null; estimated_ms: number; lines: number; name: string; quadrant: string } | null; resume?: { executed: number; homing: boolean; issue: "Unknown" | "ManualReset" | "ManualDisconnect" | "ManualAbort" | "StopResponding" | "UnexpectedReset" | "UnexpectedDisconnect" | "MachineAlarm"; sent: number; set_wco: boolean; target: number } | null };
}

interface AppView {
  /**
   * Closes a dialog (default: the top one). Closing the fire confirmation drops its laser request.
   *
   * Action `view.close_dialog`, tier `ui`.
   */
  closeDialog(params?: { name?: "settings" | "shortcuts" | "fire_confirmation" | "recover" | "welcome" | "import" | "raster" | "material_test" | "about" | null }): { changed: boolean };
  /**
   * Closes the drawer when it shows this panel.
   *
   * Action `view.close_panel`, tier `ui`.
   */
  closePanel(params: { name: "layers" | "object" | "machine" | "console" }): { changed: boolean };
  /**
   * Removes a toast.
   *
   * Action `view.dismiss_toast`, tier `ui`.
   */
  dismissToast(params: { id: number }): { changed: boolean };
  /**
   * Fits the work area in the canvas.
   *
   * Action `view.fit`, tier `ui`.
   */
  fit(params?: Record<string, unknown>): { changed: boolean };
  /**
   * Records which element has the keyboard focus.
   *
   * Action `view.focus`, tier `ui`.
   */
  focus(params?: { target?: string | null }): { changed: boolean };
  /**
   * The view: mode, tool, panel, dialogs, toasts, viewport, focus, the pending laser request.
   *
   * Action `view.get`, tier `read`.
   */
  get(params?: Record<string, unknown>): { dialogs: ("settings" | "shortcuts" | "fire_confirmation" | "recover" | "welcome" | "import" | "raster" | "material_test" | "about")[]; drawer_open: boolean; fire_request?: { action: string; countdown_ms: number; id: number; params: unknown; requested_by: string; summary: { bounds?: number[] | null; code?: string | null; estimated_ms?: number | null; lines?: number | null; program?: string | null } } | null; fit_request: number; focus?: string | null; mode: "design" | "produce"; panel: "layers" | "object" | "machine" | "console"; toasts: ({ args?: Record<string, unknown>; id: number; level: "info" | "ok" | "warning" | "error"; message: string })[]; tool: "select" | "rect" | "ellipse" | "text"; viewport?: { center: number[]; zoom: number } | null };
  /**
   * Opens a dialog on top of the others (dialogs are not modal: STOP stays reachable).
   *
   * Action `view.open_dialog`, tier `ui`.
   */
  openDialog(params: { name: "settings" | "shortcuts" | "fire_confirmation" | "recover" | "welcome" | "import" | "raster" | "material_test" | "about" }): { changed: boolean };
  /**
   * Shows a panel of the dock (`layers`, `object`, `machine`, `console`; opens the drawer in the medium layout).
   *
   * Action `view.open_panel`, tier `ui`.
   */
  openPanel(params: { name: "layers" | "object" | "machine" | "console" }): { changed: boolean };
  /**
   * Switches between design and produce (produce shows the machine panel).
   *
   * Action `view.set_mode`, tier `ui`.
   */
  setMode(params: { mode: "design" | "produce" }): { changed: boolean };
  /**
   * Chooses the Design mode tool (select, rectangle, ellipse, text).
   *
   * Action `view.set_tool`, tier `ui`.
   */
  setTool(params: { tool: "select" | "rect" | "ellipse" | "text" }): { changed: boolean };
  /**
   * Centres the view on a point (mm) at a zoom (CSS pixels per mm).
   *
   * Action `view.set_viewport`, tier `ui`.
   */
  setViewport(params: { center: number[]; zoom: number }): { changed: boolean };
  /**
   * Shows a short message (at most 3 at once); returns its id.
   *
   * Action `view.toast`, tier `ui`.
   */
  toast(params: { args?: Record<string, unknown>; level?: "info" | "ok" | "warning" | "error"; message: string }): number;
}

/** The application: one namespace per action prefix. */
declare const app: {
  readonly automation: AppAutomation;
  readonly console: AppConsole;
  readonly document: AppDocument;
  readonly fire: AppFire;
  readonly fonts: AppFonts;
  readonly history: AppHistory;
  readonly job: AppJob;
  readonly machine: AppMachine;
  readonly migrate: AppMigrate;
  readonly session: AppSession;
  readonly settings: AppSettings;
  readonly state: AppState;
  readonly view: AppView;
};

/** Writes a line to the run's log (the app console). */
declare const console: { log(...values: unknown[]): void };
