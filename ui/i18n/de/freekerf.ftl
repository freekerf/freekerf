# FreeKerf: Deutsch. Seeded from freekerf/design prototypes/app/i18n.js (e284367); used to test long strings.


## mode
mode-design = Entwerfen
mode-produce = Fertigen
mode-narrowNote = Die Bearbeitung ist ab einer Bildschirmbreite von 820 px verfügbar. Hier steuern und überwachen Sie die Maschine.

## estop
estop = STOPP
estop-hint = Sofort stoppen: schaltet den Laser ab und setzt die Steuerung zurück (Umschalt+Esc). Ersetzt nicht den physischen Not-Halt-Schalter der Maschine.

## state
state-offline = Nicht verbunden
state-connecting = Verbindungsaufbau…
state-idle = Betriebsbereit
state-run = Auftrag läuft
state-hold = Angehalten
state-alarm = Alarmzustand
state-home = Referenzfahrt
state-jog = Verfahren
state-frame = Umrissfahrt

## file
file-unsaved = ungespeicherte Änderungen

## tool
tool-select = Auswählen (V)
tool-node = Knoten bearbeiten (N)
tool-rect = Rechteck (R)
tool-ellipse = Ellipse (E)
tool-text = Text (T)
tool-import = SVG, DXF oder Bild importieren (Strg+I)
tool-trace = Bild vektorisieren
tool-weld = Verschweißen
tool-union = Vereinigung
tool-subtract = Subtraktion
tool-intersect = Schnittmenge
tool-offset = Kontur versetzen (Offset)
tool-array = Anordnung (Array)
tool-test = Materialtest
tool-undo = Rückgängig (Strg+Z)
tool-redo = Wiederholen (Strg+Umschalt+Z)
tool-camera = Kamera und Überlagerung

## view
view-zoomIn = Vergrößern (+)
view-zoomOut = Verkleinern (−)
view-fit = An Arbeitsfläche anpassen (F)
view-multi = Mehrfachauswahl
view-marquee = Bereichsauswahl

## panel
panel-layers = Ebenen
panel-object = Objekteigenschaften
panel-machine = Maschinensteuerung
panel-console = Konsole
panel-close = Bereich schließen
panel-resize-width = Ziehen, um die Bereiche anzupassen (Doppelklick: Standardgröße)
panel-resize-layers = Ziehen, um Ebenen anzupassen (Doppelklick: Standardgröße)

## layers
layers-output = An den Laser senden
layers-show = Auf der Zeichenfläche anzeigen
layers-mode = Modus
layers-mode-line = Linie
layers-mode-fill = Füllung
layers-mode-image = Bild
layers-mode-tool = Werkzeug
layers-speed = Geschwindigkeit
layers-power = Leistung
layers-passes = Durchgänge
layers-lines-per-mm = Linien pro mm
layers-lines-per-mm-hint = { $mm } mm zwischen den Schraffurlinien. Mehr Linien: dunklere, gleichmäßigere Füllung, längerer Auftrag.
layers-pattern = Muster
layers-pattern-horizontal = Waagerechte Linien
layers-pattern-vertical = Senkrechte Linien
layers-pattern-diagonal = Diagonal (45°)
layers-pattern-reverse_diagonal = Diagonal (−45°)
layers-pattern-grid = Gitter
layers-pattern-diagonal_grid = Diagonales Gitter
layers-pattern-cross = Kreuze (+)
layers-pattern-diagonal_cross = Diagonale Kreuze (×)
layers-pattern-squares = Rauten
layers-pattern-zig_zag = Zickzack
layers-pattern-hilbert = Hilbert-Kurve
layers-pattern-inset = Konzentrisch (Versatz)
layers-laser = Laser
layers-laser-constant = M3 konstant
layers-laser-dynamic = M4 dynamisch
layers-laser-hint = M3: überall dieselbe Leistung (Schneiden). M4: die Leistung folgt der Geschwindigkeit im Grbl-Lasermodus ($32=1), Ecken und Beschleunigungen brennen nicht dunkler (Gravur, Füllungen).
layers-interval = Zeilenabstand
layers-air = Luftunterstützung
layers-material = Material aus der Bibliothek übernehmen…
layers-up = In der Ausgabereihenfolge nach oben
layers-down = In der Ausgabereihenfolge nach unten
layers-empty = Noch keine Ebenen. Zeichnen Sie etwas oder wählen Sie eine Farbe in der Palette.
layers-objects = { $n } Obj.
layers-edit = Ebenenparameter bearbeiten
layers-order = Ausgabereihenfolge: von oben nach unten
layers-toolNote = Werkzeugebene: wird auf der Zeichenfläche als Hilfslinie angezeigt, aber nie an den Laser gesendet.

## palette
palette-hint = Farbe = Ebene. Bei ausgewählten Objekten tippen Sie auf eine Farbe, um die Ebene zu wechseln.

## obj
obj-none = Nichts ausgewählt. Tippen Sie auf ein Objekt oder zeichnen Sie ein neues.
obj-count = { $n } Objekte ausgewählt
obj-x = X
obj-y = Y
obj-w = Breite
obj-h = Höhe
obj-rot = Drehung
obj-lock = Seitenverhältnis beibehalten
obj-layer = Ebene
obj-text = Text
obj-size = Schriftgröße
obj-duplicate = Duplizieren (Strg+D)
obj-delete = Löschen (Entf)
obj-front = In den Vordergrund
obj-back = In den Hintergrund
obj-align = Ausrichten
obj-alignLeft = Links ausrichten
obj-alignCenterH = Horizontal zentrieren
obj-alignRight = Rechts ausrichten
obj-alignTop = Oben ausrichten
obj-alignMiddleV = Vertikal zentrieren
obj-alignBottom = Unten ausrichten
obj-combine = Kombinieren
obj-offset = Versatzkontur
obj-offset-distance = Abstand
obj-offset-go = Kontur hinzufügen
obj-array = Anordnung
obj-array-columns = Spalten
obj-array-rows = Zeilen
obj-array-gap = Abstand
obj-array-go = Kopien erstellen
obj-type-rect = Rechteck
obj-type-ellipse = Ellipse
obj-type-text = Text
obj-type-path = Pfad
obj-type-image = Bild
obj-image = Bildeinstellungen
obj-brightness = Helligkeit
obj-contrast = Kontrast
obj-dither = Rasterung

## machine
machine-emulator = FreeKerf-Emulator (ohne Maschine)
machine-refreshPorts = Erneut nach Anschlüssen suchen
machine-openGcode = G-Code öffnen…
machine-noProgram = Kein Programm geladen
machine-lines = Zeilen
machine-overrides = Echtzeitanpassungen
machine-ovFeed = Geschwindigkeit
machine-ovPower = Leistung
machine-ovRapid = Eilgangbewegungen
machine-noButtons = Noch keine benutzerdefinierten Schaltflächen.
machine-laserLife = Nutzung des Lasermoduls
machine-connection = Verbindung
machine-port = Anschluss
machine-connect = Verbinden
machine-disconnect = Verbindung trennen
machine-detected = { $fw } erkannt · Arbeitsbereich { $w }×{ $h } mm
machine-position = Position
machine-jog = Verfahren (Jog)
machine-step = Schrittweite
machine-continuous = Kontinuierlich (zum Verfahren gedrückt halten)
machine-jogSpeed = Geschwindigkeit
machine-home = Referenzfahrt
machine-unlock = Entsperren
machine-setOrigin = Nullpunkt hier
machine-goOrigin = Zum Nullpunkt
machine-frame = Umriss abfahren
machine-frameHint = Fährt das Rechteck des Auftrags mit ausgeschaltetem Laser ab.
machine-jobOrigin = Auftragsursprung
machine-jobOrigin-abs = Absolute Koordinaten
machine-jobOrigin-current = Aktuelle Position
machine-jobOrigin-user = Benutzerdefinierter Nullpunkt
machine-anchor = Ankerpunkt
machine-start = Starten
machine-pause = Pausieren
machine-resume = Fortsetzen
machine-stop = Auftrag abbrechen
machine-job = Auftrag
job-start = Auftragsstart
job-start-absolute = Absolut
job-start-current_position = Aktuelle Position
job-start-user_origin = Benutzerursprung
job-anchor = Anker
job-anchor-top_left = Oben links
job-anchor-top = Oben
job-anchor-top_right = Oben rechts
job-anchor-left = Links
job-anchor-center = Mitte
job-anchor-right = Rechts
job-anchor-bottom_left = Unten links
job-anchor-bottom = Unten
job-anchor-bottom_right = Unten rechts
job-design-go = Auftrag aus dem Entwurf erstellen
machine-elapsed = Verstrichen
machine-remaining = Verbleibend
machine-estimate = Geschätzte Dauer
machine-layerNow = Aktuelle Ebene
machine-line = Zeile { $a } von { $b }
machine-custom = Benutzerdefinierte Schaltflächen
machine-addButton = Neue Schaltfläche
machine-simSpeed = Simulationsgeschwindigkeit
machine-simulate = Fehler simulieren
machine-simAlarm = Endschalter-Alarm
machine-simDisconnect = Verbindungsabbruch
machine-notConnected = Verbinden Sie die Maschine, um zu verfahren, den Umriss abzufahren oder zu gravieren.
machine-noJob = Nichts zu gravieren: Keine Ebene ist zum Senden an den Laser markiert.

## console
console-placeholder = G-Code oder Befehl eingeben ($$, $H, ?)…
console-send = Senden
console-hideStatus = Statusberichte ausblenden

## preflight
preflight-title = Bereit zum Gravieren?
preflight-summary = { $n } Ebenen · { $t } · { $w }×{ $h } mm
preflight-c1 = Schutzbrille passend zu diesem Laser
preflight-c2 = Absaugung und Luftunterstützung eingeschaltet
preflight-c3 = Material fixiert und Fokus eingestellt
preflight-c4 = Feuerlöscher griffbereit; Maschine nie unbeaufsichtigt lassen
preflight-fire = Laser starten
preflight-wait = Start möglich in { $s } s
preflight-cancel = Abbrechen
preflight-note = Sie können jederzeit mit Esc pausieren oder mit STOPP alles anhalten.

## alarm
alarm-title = Alarm { $n }
alarm-body = Die Steuerung hat die Bewegung gestoppt und den Laser in Zeile { $n } abgeschaltet. Prüfen Sie, ob der Laserkopf frei beweglich ist, bevor Sie fortfahren.

## disc
disc-title = Verbindung unterbrochen
disc-body = Die Maschine reagiert seit Zeile { $n } nicht mehr. Die Steuerung hat den Laser abgeschaltet.

## recover
recover-restoreWco = Werkstücknullpunkt des Auftrags wiederherstellen
recover-nothing = Kein unterbrochener Auftrag.
recover-open = Auftrag wiederherstellen…
recover-title = Auftrag wiederherstellen
recover-cause = Ursache
recover-fromStart = Von Anfang an
recover-fromBefore = Einige Zeilen vor der Unterbrechung (Zeile { $n })
recover-recommended = empfohlen
recover-fromSent = Ab der zuletzt gesendeten Zeile ({ $n })
recover-fromLine = Ab Zeile
recover-homing = Vor dem Fortsetzen eine Referenzfahrt ($H) durchführen
recover-wco = Werkstücknullpunkt X{ $x } Y{ $y } wiederherstellen
recover-go = Von hier fortsetzen
recover-dismiss = Jetzt nicht

## toast
toast-layerChanged = Auf Ebene { $c } verschoben.
toast-jobDone = Auftrag in { $t } abgeschlossen.
toast-stopped = Auftrag abgebrochen. Laser ausgeschaltet.
toast-estop = STOPP: Laser aus und Steuerung zurückgesetzt. Zum Fortfahren bitte entsperren.
toast-connected = Verbunden: { $fw }
toast-frameDone = Umrissfahrt abgeschlossen.
toast-material = Material „{ $m }“ auf Ebene { $c } übernommen.
toast-unlocked = Entsperrt. Prüfen Sie die Position vor dem Verfahren.
toast-homed = Referenzfahrt abgeschlossen.
toast-origin = Werkstücknullpunkt hier gesetzt.

## settings
settings-tab-display = Anzeige
settings-tab-machine = Maschine
settings-tab-job = Auftragscode
settings-tab-safety = Sicherheit
settings-tab-automation = Automatisierung
settings-langAuto = Automatisch (Systemsprache)
settings-firmware = Firmware
settings-baud = Übertragungsgeschwindigkeit (Baud)
settings-streaming = Übertragungsmodus
settings-streaming-Buffered = Gepuffert (schnell)
settings-streaming-Synchronous = Synchron (sicher)
settings-streaming-RepeatOnError = Bei Fehler wiederholen
settings-statusMs = Statusabfrage alle (ms)
settings-resetOnConnect = Steuerung beim Verbinden zurücksetzen
settings-supportPwm = Hardware-PWM
settings-autoCooling = Automatische Abkühlpausen
settings-jogSpeed = Verfahrgeschwindigkeit (mm/min)
settings-header = Kopfzeilen-Code
settings-passes = Zwischen den Durchgängen
settings-footer = Fußzeilen-Code
settings-jobHint = G-Code, der vor dem Auftrag, zwischen den Durchgängen und danach gesendet wird. [expr] wird ausgewertet; !, ~, ?, ctrl-x und 0xNN werden sofort gesendet.
settings-countdown = Countdown vor „Laser auslösen“
settings-userOnly = Nur Sie können das ändern, hier oder mit `freekerf config set`: Skripte und Agenten nicht.
settings-mcp = KI-Agenten (MCP-Server)
settings-mcpMachine = Agenten dürfen die Maschine bewegen
settings-on = Ein
settings-off = Aus
settings-automationNote = Standardmäßig aus. Agenten feuern den Laser nie: weder mit einer Berechtigung noch nach einer Bestätigung.
settings-title = Darstellung und Eingabe
settings-theme = Farbschema

## theme
theme-dark = Dunkel
theme-light = Hell

## settings
settings-contrast = Hoher Kontrast
settings-density = Bedienelementdichte

## density
density-auto = Automatisch
density-compact = Kompakt (Maus)
density-comfortable = Komfortabel (Touchbedienung)

## settings
settings-lang = Sprache
settings-units = Maßeinheiten
settings-welcome = Willkommensbildschirm erneut anzeigen
settings-shortcuts = Tastenkombinationen

## status
status-zoom = Zoom
status-head = Laserkopf
status-sel = Auswahl

## units
units-mm = mm
units-in = Zoll

## welcome
welcome-title = Willkommen bei FreeKerf
welcome-sub = Freies Design und freie Steuerung für Ihre Lasermaschine.
welcome-setup = Meine Maschine einrichten
welcome-migrate = Aus LaserGRBL importieren
welcome-explore = Mit dem Simulator erkunden
welcome-origin = Weiterentwicklung von LaserGRBL, ursprünglich von Diego Settimi. Freie Software unter GPL-3.0-or-later.
welcome-kindTitle = Welche Maschine besitzen Sie?
welcome-kind-diode = Diodenlaser
welcome-kind-diodeSub = GRBL oder grblHAL, 5–40 W optisch
welcome-kind-co2 = CO₂-Laser
welcome-kind-co2Sub = GRBL, grblHAL, Smoothie oder Marlin
welcome-kind-other = Andere oder generische Maschine
welcome-kind-otherSub = Plotter, leichte Fräse, eigene Firmware
welcome-area = Arbeitsbereich
welcome-next = Weiter
welcome-back = Zurück
welcome-finish = Loslegen
welcome-connTitle = Wir suchen Ihre Maschine
welcome-found = Gefunden:
welcome-searching = USB-Anschlüsse und Netzwerk werden durchsucht…
welcome-migrateTitle = Aus LaserGRBL importieren
welcome-migrateNote = Eine LaserGRBL-Installation wurde gefunden. Die Originaldateien werden nicht verändert.
welcome-migrateGo = Ausgewählte importieren
welcome-ready = Alles bereit
welcome-readySub = Ein Beispielprojekt ist geöffnet. Probieren Sie Verschieben, Drehen und Farbwechsel aus.

## shortcuts
shortcuts-title = Tastenkombinationen

## common
common-skip-canvas = Zur Zeichenfläche springen
common-soon = Kommt mit einem späteren Update.
common-close = Schließen
common-ok = OK
common-menu = Menü

## ctx
ctx-title = Aktionen

## jog
jog-n = Nach hinten verfahren (Y+)
jog-s = Nach vorne verfahren (Y−)
jog-w = Nach links verfahren (X−)
jog-e = Nach rechts verfahren (X+)

## drawer
drawer-toggle = Bereiche

## test
test-title = Materialtest
test-desc = Raster aus Leistung × Geschwindigkeit, um die Materialparameter zu ermitteln.

## toast
toast-needConn = Bitte zuerst die Maschine verbinden.
toast-needSel = Bitte zuerst ein oder mehrere Objekte auswählen.

## estop
estop-title = Stopp ausgelöst
estop-body = Laser aus und Steuerung in Zeile { $n } zurückgesetzt. Die Position kann verloren sein: vor dem Fortsetzen eine Referenzfahrt durchführen.

## cause
cause-other = unterbrochen
cause-alarm = Alarm der Steuerung
cause-disc = Verbindung unterbrochen
cause-estop = manueller Stopp (STOPP)

## obj
obj-dither-fs = Floyd–Steinberg
obj-dither-atk = Atkinson
obj-dither-thr = Schwellwert
obj-center = Position bezogen auf die Objektmitte

## welcome
welcome-skip = Überspringen

## sb
sb-auto = automatisch

## kb
kb-g-tools = Werkzeuge
kb-g-edit = Bearbeiten
kb-g-view = Ansicht
kb-g-machine = Maschine
kb-undo = Rückgängig
kb-redo = Wiederholen
kb-copy = Kopieren
kb-cut = Ausschneiden
kb-paste = Einfügen
kb-clipboard = Kopieren · ausschneiden · einfügen
kb-combine = Verbinden
kb-split = Trennen
kb-combine-split = Verbinden · trennen
kb-dup = Duplizieren
kb-del = Löschen
kb-all = Alles auswählen
kb-nudge = Auswahl um 1 mm verschieben (Umschalt: 10 mm, Alt: 0,1 mm)
kb-constrain = Beim Ziehen: Umschalt sperrt Achse/Seitenverhältnis; Alt skaliert von der Mitte aus
kb-esc = Werkzeug abbrechen oder Auswahl aufheben
kb-zoom = Vergrößern / Verkleinern
kb-fit = An Arbeitsfläche anpassen
kb-pan = Ansicht verschieben (gedrückt halten und ziehen)
kb-wheel = Zoom an der Mausposition (Umschalt: horizontal blättern)
kb-mode = Entwerfen / Fertigen
kb-start = Starten (öffnet die Bestätigung)
kb-pause = Pausieren / Fortsetzen
kb-frame = Umriss abfahren
kb-hold = Laufenden Auftrag pausieren
kb-estop = STOPP (funktioniert überall)

## design (M4a, on the UI contract)
layers-mode-fill_and_line = Füllen und Kontur
layers-mode-cut = Schneiden
import-title = Importieren
import-file = Datei (SVG, DXF oder Bild)
import-colors = Farben der Datei
import-colors-keep = Farben behalten: eine Ebene pro Farbe
import-colors-unify = Alles auf die aktuelle Ebene legen
import-go = Importieren
import-done = { $objects } Objekte importiert ({ $layers } neue Ebenen).
welcome-migrateSub = Verbindung, benutzerdefinierte Schaltflächen, Materialien und Zähler
migrate-none = Auf diesem Computer wurde keine LaserGRBL-Installation gefunden.
migrate-connection = Verbindung: Anschluss, Baudrate, Firmware und Streaming-Modus
migrate-buttons = { $n } benutzerdefinierte Schaltflächen
migrate-materials = Benutzermaterialien (UserMaterials.psh)
migrate-counters = Nutzungszähler von { $n } Lasermodulen
migrate-preferences = { $n } weitere Einstellungen: Joggen, Kühlung, Kopf- und Fußzeilen des Auftrags

## raster
raster-title = Bildgravur
raster-none = Wählen Sie ein Bild, um festzulegen, wie es graviert wird.
raster-tool = Methode
raster-tool-line_to_line = Graustufen
raster-tool-dithering = Rasterung
raster-tool-passthrough = Unverändert
raster-tool-vectorize = Konturen nachzeichnen
raster-tool-centerline = Mittellinien
raster-dither-random = Zufällig
raster-lines = Auflösung
raster-speed = Geschwindigkeit
raster-minPower = Minimale Leistung
raster-maxPower = Maximale Leistung
raster-direction = Richtung
raster-dir-horizontal = Horizontal
raster-dir-vertical = Vertikal
raster-dir-diagonal = Diagonal
raster-preview = Vorschau der Gravur
raster-apply = Anwenden
obj-image-open = Bildgravur…

## material test
mtest-title = Materialtest
mtest-mode = Zellen
mtest-mode-fill = Gefüllt (Gravur)
mtest-mode-line = Umrisse (Linien, Schnitte)
mtest-power = Leistung, je Zeile
mtest-speed = Geschwindigkeit, je Spalte
mtest-from = Von
mtest-to = Bis
mtest-steps = Schritte
mtest-cell = Zellengröße
mtest-cells = { $n } Zellen, jede auf einer eigenen Ebene. Die Beschriftung wird mit geringer Leistung graviert.
mtest-go = Test zeichnen
mtest-done = Materialtest gezeichnet: { $n } Zellen.

## automation
perm-title = Berechtigung
perm-agent = Der Agent „{ $name }“ möchte { $action } ausführen.
perm-macro = Das Makro „{ $name }“ möchte { $action } ausführen.
perm-tier = Dafür ist die Berechtigung „{ $tier }“ nötig. Laseraktionen werden hier nie erlaubt.
perm-deny = Ablehnen
perm-run = Dieses Mal
perm-session = Bis ich die App schließe
perm-always = Immer
auto-agents = { $n ->
    [one] Agent verbunden
   *[other] { $n } Agenten verbunden
}
auto-script = Läuft: { $name }
auto-cancel = Skript abbrechen
settings-grants = Erteilte Berechtigungen
settings-grantsNone = Keine.
settings-revoke = Widerrufen
settings-log = Letzte Aktionen von Skripten und Agenten
settings-logNone = Noch nichts.
settings-refused = abgelehnt
