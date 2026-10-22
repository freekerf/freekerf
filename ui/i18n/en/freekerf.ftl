# FreeKerf: English (reference for keys). UI text of the desktop app.


## mode
mode-design = Design
mode-produce = Produce
mode-narrowNote = Editing is available on screens from 820 px wide. Here you control and watch the machine.

## estop
estop = STOP
estop-hint = Stop now: turns the laser off and resets the controller (Shift+Esc). It does not replace the physical emergency stop of the machine.

## state
state-offline = Disconnected
state-connecting = Connecting…
state-idle = Ready
state-run = Running
state-hold = Paused
state-alarm = Alarm
state-home = Homing
state-jog = Moving
state-frame = Framing

## file
file-unsaved = unsaved changes

## tool
tool-select = Select (V)
tool-node = Edit nodes (N)
tool-rect = Rectangle (R)
tool-ellipse = Ellipse (E)
tool-text = Text (T)
tool-import = Import SVG, DXF or image (Ctrl+I)
tool-trace = Trace image
tool-weld = Weld
tool-union = Union
tool-subtract = Subtract
tool-intersect = Intersect
tool-offset = Offset outline
tool-array = Array
tool-test = Material test
tool-undo = Undo (Ctrl+Z)
tool-redo = Redo (Ctrl+Shift+Z)
tool-camera = Camera and overlay

## view
view-zoomIn = Zoom in (+)
view-zoomOut = Zoom out (−)
view-fit = Fit the table (F)
view-multi = Multiple selection
view-marquee = Area selection

## panel
panel-layers = Layers
panel-object = Object
panel-machine = Machine
panel-console = Console
panel-close = Close panel
panel-resize-width = Drag to resize the panels (double-click: default size)
panel-resize-layers = Drag to resize Layers (double-click: default size)

## layers
layers-output = Send to laser
layers-show = Show on canvas
layers-mode = Mode
layers-mode-line = Line
layers-mode-fill = Fill
layers-mode-image = Image
layers-mode-tool = Tool
layers-speed = Speed
layers-power = Power
layers-passes = Passes
layers-lines-per-mm = Lines per mm
layers-lines-per-mm-hint = { $mm } mm between hatch lines. More lines: darker, smoother filling, longer job.
layers-pattern = Pattern
layers-pattern-horizontal = Horizontal lines
layers-pattern-vertical = Vertical lines
layers-pattern-diagonal = Diagonal (45°)
layers-pattern-reverse_diagonal = Diagonal (−45°)
layers-pattern-grid = Grid
layers-pattern-diagonal_grid = Diagonal grid
layers-pattern-cross = Crosses (+)
layers-pattern-diagonal_cross = Diagonal crosses (×)
layers-pattern-squares = Diamonds
layers-pattern-zig_zag = Zig-zag
layers-pattern-hilbert = Hilbert curve
layers-pattern-inset = Concentric (inset)
layers-laser = Laser
layers-laser-constant = M3 constant
layers-laser-dynamic = M4 dynamic
layers-laser-hint = M3: the same power everywhere (cutting). M4: power follows the speed in Grbl laser mode ($32=1), so corners and accelerations do not burn darker (engraving, fillings).
layers-interval = Line interval
layers-air = Air assist
layers-material = Apply a material from the library…
layers-up = Move up in the output order
layers-down = Move down in the output order
layers-empty = No layers yet. Draw something or pick a colour in the palette.
layers-objects = { $n } obj.
layers-edit = Edit layer settings
layers-order = Output order: top to bottom
layers-toolNote = Tool layer: shown on the canvas as a guide, never sent to the laser.

## palette
palette-hint = Colour = layer. With objects selected, pick a colour to change their layer.

## obj
obj-none = Nothing selected. Pick an object or draw a new one.
obj-count = { $n } objects selected
obj-x = X
obj-y = Y
obj-w = Width
obj-h = Height
obj-rot = Rotation
obj-lock = Keep proportions
obj-layer = Layer
obj-text = Text
obj-size = Size
obj-duplicate = Duplicate (Ctrl+D)
obj-delete = Delete (Del)
obj-front = Bring to front
obj-back = Send to back
obj-align = Align
obj-alignLeft = Align left
obj-alignCenterH = Centre horizontally
obj-alignRight = Align right
obj-alignTop = Align top
obj-alignMiddleV = Centre vertically
obj-alignBottom = Align bottom
obj-combine = Combine
obj-offset = Offset outline
obj-offset-distance = Distance
obj-offset-go = Add outline
obj-array = Array
obj-array-columns = Columns
obj-array-rows = Rows
obj-array-gap = Gap
obj-array-go = Make copies
obj-type-rect = Rectangle
obj-type-ellipse = Ellipse
obj-type-text = Text
obj-type-path = Path
obj-type-image = Image
obj-image = Image adjustments
obj-brightness = Brightness
obj-contrast = Contrast
obj-dither = Dithering

## machine
machine-emulator = FreeKerf emulator (no machine)
machine-refreshPorts = Look for ports again
machine-portDenied = You don't have permission to use { $port }. Add yourself to the { $group } group, then log out and back in.
machine-portDeniedCopy = Copy the command
machine-portDeniedCopied = Copied
machine-openGcode = Open G-code…
machine-noProgram = No program loaded
machine-lines = Lines
machine-overrides = Overrides
machine-ovFeed = Speed
machine-ovPower = Power
machine-ovRapid = Rapid moves
machine-noButtons = No custom buttons yet.
machine-laserLife = Laser module usage
machine-connection = Connection
machine-port = Port
machine-connect = Connect
machine-disconnect = Disconnect
machine-detected = { $fw } detected · area { $w }×{ $h } mm
machine-position = Position
machine-jog = Jog
machine-step = Step
machine-continuous = Continuous (hold to move)
machine-jogSpeed = Speed
machine-home = Home
machine-unlock = Unlock
machine-setOrigin = Origin here
machine-goOrigin = Go to origin
machine-frame = Frame
machine-frameHint = Runs around the job rectangle with the laser off.
machine-jobOrigin = Job start
machine-jobOrigin-abs = Absolute coordinates
machine-jobOrigin-current = Current position
machine-jobOrigin-user = User origin
machine-anchor = Anchor point
machine-start = Start
machine-pause = Pause
machine-resume = Resume
machine-stop = Stop job
machine-job = Job
job-start = Job start
job-start-absolute = Absolute
job-start-current_position = Current position
job-start-user_origin = User origin
job-anchor = Anchor
job-anchor-top_left = Top left
job-anchor-top = Top
job-anchor-top_right = Top right
job-anchor-left = Left
job-anchor-center = Centre
job-anchor-right = Right
job-anchor-bottom_left = Bottom left
job-anchor-bottom = Bottom
job-anchor-bottom_right = Bottom right
job-design-go = Make the job from the design
machine-elapsed = Elapsed
machine-remaining = Remaining
machine-estimate = Estimate
machine-layerNow = Current layer
machine-line = Line { $a } of { $b }
machine-custom = Custom buttons
machine-addButton = New button
machine-simSpeed = Simulation speed
machine-simulate = Simulate a failure
machine-simAlarm = Limit alarm
machine-simDisconnect = Lost connection
machine-notConnected = Connect the machine to jog, frame or burn.
machine-noJob = Nothing to burn: no layer is set to be sent to the laser.

## console
console-placeholder = Type G-code or a command ($$, $H, ?)…
console-send = Send
console-hideStatus = Hide status reports

## preflight
preflight-title = Ready to burn?
preflight-summary = { $n } layers · { $t } · { $w }×{ $h } mm
preflight-c1 = Safety glasses right for this laser
preflight-c2 = Exhaust and air assist on
preflight-c3 = Material held down and focus set
preflight-c4 = Fire extinguisher nearby; do not leave the machine alone
preflight-fire = Fire laser
preflight-wait = Fire in { $s } s
preflight-cancel = Cancel
preflight-note = You can pause at any time with Esc or stop everything with STOP.

## alarm
alarm-title = Alarm { $n }
alarm-body = The controller stopped the motion and turned the laser off at line { $n }. Check that nothing blocks the head before you go on.

## disc
disc-title = Connection lost
disc-body = The machine stopped answering at line { $n }. The controller turned the laser off.

## recover
recover-restoreWco = Restore the work origin of the job
recover-nothing = No interrupted job.
recover-open = Recover job…
recover-title = Recover job
recover-cause = Cause
recover-fromStart = From the start
recover-fromBefore = A few lines before the interruption (line { $n })
recover-recommended = recommended
recover-fromSent = From the last line sent ({ $n })
recover-fromLine = From line
recover-homing = Home ($H) before going on
recover-wco = Restore work origin X{ $x } Y{ $y }
recover-go = Go on from here
recover-dismiss = Not now

## toast
toast-layerChanged = Moved to layer { $c }.
toast-jobDone = Job done in { $t }.
toast-stopped = Job stopped. Laser off.
toast-estop = STOP: laser off and controller reset. Unlock to go on.
toast-connected = Connected: { $fw }
toast-frameDone = Frame done.
toast-material = Material “{ $m }” applied to layer { $c }.
toast-unlocked = Unlocked. Check the position before moving.
toast-homed = Homing done.
toast-origin = Work origin set here.

## settings
settings-tab-display = Display
settings-tab-machine = Machine
settings-tab-job = Job code
settings-tab-safety = Safety
settings-tab-automation = Automation
settings-langAuto = Automatic (system language)
settings-firmware = Firmware
settings-baud = Speed (baud)
settings-streaming = Streaming
settings-streaming-Buffered = Buffered (fast)
settings-streaming-Synchronous = Synchronous (safe)
settings-streaming-RepeatOnError = Repeat on error
settings-statusMs = Status every (ms)
settings-resetOnConnect = Reset the board when connecting
settings-supportPwm = Hardware PWM
settings-autoCooling = Automatic cooling pauses
settings-jogSpeed = Jog speed (mm/min)
settings-header = Header
settings-passes = Between passes
settings-footer = Footer
settings-jobHint = G-code sent before the job, between passes and after it. [expr] is evaluated; !, ~, ?, ctrl-x and 0xNN are sent at once.
settings-countdown = Countdown before "Fire laser"
settings-userOnly = Only you can change this, here or with `freekerf config set`: scripts and agents cannot.
settings-mcp = AI agents (MCP server)
settings-mcpMachine = Agents may move the machine
settings-on = On
settings-off = Off
settings-automationNote = Off by default. Agents never fire the laser: not even with a grant, not even after a confirmation.
settings-title = Display and input
settings-theme = Theme

## theme
theme-dark = Dark
theme-light = Light

## settings
settings-contrast = High contrast
settings-density = Density

## density
density-auto = Automatic
density-compact = Compact (mouse)
density-comfortable = Comfortable (touch)

## settings
settings-lang = Language
settings-units = Units
settings-welcome = Show the welcome again
settings-shortcuts = Keyboard shortcuts

## status
status-zoom = Zoom
status-head = Head
status-sel = Selection

## units
units-mm = mm
units-in = in

## welcome
welcome-title = Welcome to FreeKerf
welcome-sub = Free design and control for your laser machine.
welcome-setup = Set up my machine
welcome-migrate = Import from LaserGRBL
welcome-explore = Explore with the simulator
welcome-origin = The successor of LaserGRBL, created by Diego Settimi. Free software under GPL-3.0-or-later.
welcome-kindTitle = What machine do you have?
welcome-kind-diode = Diode laser
welcome-kind-diodeSub = GRBL or grblHAL, 5–40 W optical
welcome-kind-co2 = CO₂ laser
welcome-kind-co2Sub = GRBL, grblHAL, Smoothie or Marlin
welcome-kind-other = Other or generic
welcome-kind-otherSub = Plotter, light mill, own firmware
welcome-area = Work area
welcome-next = Next
welcome-back = Back
welcome-finish = Start
welcome-connTitle = Let us find the machine
welcome-found = We found:
welcome-searching = Looking for USB ports and the network…
welcome-migrateTitle = Import from LaserGRBL
welcome-migrateNote = We found a LaserGRBL installation. The original files are never changed.
welcome-migrateGo = Import selected
welcome-ready = All set
welcome-readySub = An example project is open. Try dragging, rotating and changing colours.

## shortcuts
shortcuts-title = Keyboard shortcuts

## common
common-skip-canvas = Skip to the canvas
common-soon = Coming in a later update.
common-close = Close
common-ok = OK
common-menu = Menu

## ctx
ctx-title = Actions

## jog
jog-n = Move back (Y+)
jog-s = Move forward (Y−)
jog-w = Move left (X−)
jog-e = Move right (X+)

## drawer
drawer-toggle = Panels

## test
test-title = Material test
test-desc = Power × speed grid to find the settings of a material.

## toast
toast-needConn = Connect the machine first.
toast-needSel = Select one or more objects first.

## estop
estop-title = Stop pressed
estop-body = Laser off and controller reset at line { $n }. The position may be lost: home before going on.

## cause
cause-other = interrupted
cause-alarm = controller alarm
cause-disc = lost connection
cause-estop = manual stop (STOP)

## obj
obj-dither-fs = Floyd–Steinberg
obj-dither-atk = Atkinson
obj-dither-thr = Threshold
obj-center = Position by the object centre

## welcome
welcome-skip = Skip

## sb
sb-auto = auto

## kb
kb-g-tools = Tools
kb-g-edit = Editing
kb-g-view = View
kb-g-machine = Machine
kb-undo = Undo
kb-redo = Redo
kb-copy = Copy
kb-cut = Cut
kb-paste = Paste
kb-clipboard = Copy · cut · paste
kb-combine = Join
kb-split = Split
kb-combine-split = Join · split
kb-edit-text = Edit the text
kb-dup = Duplicate
kb-del = Delete
kb-all = Select all
kb-nudge = Move the selection 1 mm (Shift: 10 mm, Alt: 0.1 mm)
kb-constrain = While dragging: Shift locks the axis or proportions; Alt resizes from the centre
kb-esc = Cancel the tool or clear the selection
kb-zoom = Zoom in / out
kb-fit = Fit the table
kb-pan = Move the view (hold and drag)
kb-wheel = Zoom at the cursor (Shift: scroll horizontally)
kb-mode = Design / Produce
kb-start = Start (opens the confirmation)
kb-pause = Pause / resume
kb-frame = Frame
kb-hold = Pause the running job
kb-estop = STOP (works everywhere)

## design (M4a, on the UI contract)
layers-mode-fill_and_line = Fill and line
layers-mode-cut = Cut
import-title = Import
import-file = File (SVG, DXF or image)
import-colors = Colours of the file
import-colors-keep = Keep the colours: one layer per colour
import-colors-unify = Put everything on the current layer
import-go = Import
import-done = Imported { $objects } objects ({ $layers } new layers).
welcome-migrateSub = Connection, custom buttons, materials and counters
migrate-none = No LaserGRBL installation was found on this computer.
migrate-connection = Connection: port, speed, firmware and streaming mode
migrate-buttons = { $n } custom buttons
migrate-materials = User materials (UserMaterials.psh)
migrate-counters = Usage counters of { $n } laser modules
migrate-preferences = { $n } other settings: jog, cooling, job header and footer

## raster
raster-title = Image engraving
raster-none = Select one image to set how it is engraved.
raster-tool = Method
raster-tool-line_to_line = Grey levels
raster-tool-dithering = Dithering
raster-tool-passthrough = As it is
raster-tool-vectorize = Trace outlines
raster-tool-centerline = Centre lines
raster-dither-random = Random
raster-lines = Resolution
raster-speed = Speed
raster-minPower = Minimum power
raster-maxPower = Maximum power
raster-direction = Direction
raster-dir-horizontal = Horizontal
raster-dir-vertical = Vertical
raster-dir-diagonal = Diagonal
raster-preview = Preview of the engraving
raster-apply = Apply
obj-image-open = Image engraving…

## material test
mtest-title = Material test
mtest-mode = Cells
mtest-mode-fill = Filled (engraving)
mtest-mode-line = Outlines (lines, cuts)
mtest-power = Power, by row
mtest-speed = Speed, by column
mtest-from = From
mtest-to = To
mtest-steps = Steps
mtest-cell = Cell size
mtest-cells = { $n } cells, each on a layer of its own. Labels are engraved at low power.
mtest-go = Draw the test
mtest-done = Material test drawn: { $n } cells.

## automation
perm-title = Permission
perm-agent = The agent “{ $name }” wants to run { $action }.
perm-macro = The macro “{ $name }” wants to run { $action }.
perm-tier = It needs the “{ $tier }” permission. Laser actions are never granted here.
perm-deny = Deny
perm-run = This time
perm-session = Until I close the app
perm-always = Always
auto-agents = { $n ->
    [one] Agent connected
   *[other] { $n } agents connected
}
auto-script = Running: { $name }
auto-cancel = Cancel the script
settings-grants = Permissions given
settings-grantsNone = None.
settings-revoke = Revoke
settings-log = Recent actions of scripts and agents
settings-logNone = Nothing yet.
settings-refused = refused

## text
text-size-hint = Height of the capital letters
text-font = Font
text-font-missing = { $name } (not installed here)
text-fonts-single = Single-line (one pass per stroke)
text-fonts-outline = Installed (outlines)
text-sample = Sample of { $name }
text-style = Style
text-bold = Bold
text-italic = Italic
text-underline = Underline
text-strikethrough = Strikethrough
text-align-left = Align lines left
text-align-center = Centre lines
text-align-right = Align lines right
text-align-justify = Justify (on a path: fill it)
text-line-spacing = Lines
text-letter-spacing = Letters
text-width = Width
text-effects = Effects
text-weld = Weld overlapping letters
text-weld-hint = Merges letters that touch into one shape, for cutting script fonts
text-outline-only = Only for outline fonts
text-outline = Outline
text-stencil = Stencil bridges
text-zero-off = 0 turns it off
text-arc = Arc radius
text-arc-hint = Bends the text on a circle (negative: along its bottom); 0 keeps it straight
text-on-path = On a path (follows it)
text-on-path-copy = On a path (the shape was deleted)
text-path-offset = Start
text-path-distance = Distance
text-path-flip = Other side
text-off-path = Take off the path
text-to-paths = Convert to paths
text-to-paths-hint = The letters become plain shapes; the text is no longer editable
text-put-on-path = Put text on path
text-on-path-hint = The text follows the shape's path, and moves with it
