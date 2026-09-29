import { Extension } from 'resource:///org/gnome/shell/extensions/extension.js';
import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

const BINARY_NAME = 'spotlight-files';
const CARD_WIDTH = 680;
const MAX_RESULTS_HEIGHT = 380;
const ROW_HEIGHT = 56;
const DEBOUNCE_MS = 150;
const BLUR_RADIUS = 30;
const BLUR_BRIGHTNESS = 0.65;

const _DBG = (msg) => log('[spotlight-ext] ' + msg);

export default class SpotlightExtension extends Extension {
    enable() {
        _DBG('enable');
        this._visible = false;
        this._selectedIndex = -1;
        this._resultRows = [];
        this._debounceId = 0;
        this._dbusId = 0;

        this._binaryPath = GLib.find_program_in_path(BINARY_NAME);
        if (!this._binaryPath) {
            _DBG('WARNING: binary not found in PATH');
        }

        this._buildUi();
        this._registerDbus();
    }

    disable() {
        _DBG('disable');
        this._hide(true);
        this._unregisterDbus();
        this._destroyUi();
    }

    // ── UI construction ────────────────────────────────────────────

    _buildUi() {
        // Container fills the screen, captures clicks for dismiss.
        this._container = new Clutter.Actor({
            name: 'spotlight-container',
            reactive: true,
        });

        // Backdrop: blurred + dimmed, covers all monitors.
        this._backdrop = new Clutter.Actor({
            name: 'spotlight-backdrop',
            reactive: true,
        });
        try {
            let mode = (Shell.BlurMode && Shell.BlurMode.BACKGROUND) ?? 1;
            this._backdrop.add_effect(new Shell.BlurEffect({
                mode: mode,
                radius: BLUR_RADIUS,
                brightness: BLUR_BRIGHTNESS,
            }));
            _DBG('blur effect added (BACKGROUND mode)');
        } catch (e) {
            _DBG('blur effect error: ' + e.message);
        }
        this._backdrop.connect('button-press-event', () => {
            this._hide();
            return Clutter.EVENT_STOP;
        });

        // Card
        this._card = new St.BoxLayout({
            name: 'spotlight-card',
            style_class: 'spotlight-card',
            vertical: true,
            reactive: true,
            width: CARD_WIDTH,
        });

        // Search bar
        let searchBar = new St.BoxLayout({
            style_class: 'spotlight-search-bar',
        });
        let magnifier = new St.Icon({
            icon_name: 'system-search',
            icon_size: 22,
            y_align: Clutter.ActorAlign.CENTER,
            style_class: 'spotlight-magnifier',
        });
        this._entry = new St.Entry({
            style_class: 'spotlight-search-entry',
            hint_text: 'Search apps, files, or calculate…',
            can_focus: true,
            x_expand: true,
        });
        this._entry.clutter_text.connect('text-changed', () => this._onTextChanged());
        this._entry.clutter_text.connect('key-press-event', (_a, event) => this._onKeyPress(event));
        searchBar.add_child(magnifier);
        searchBar.add_child(this._entry);

        // Separator
        let separator = new St.Widget({
            style_class: 'spotlight-separator',
        });

        // Results
        this._scrollView = new St.ScrollView({
            style_class: 'spotlight-scroll',
            overlay_scrollbars: true,
            vscrollbar_policy: St.PolicyType.AUTOMATIC,
            hscrollbar_policy: St.PolicyType.NEVER,
        });
        this._resultsBox = new St.BoxLayout({
            style_class: 'spotlight-results',
            vertical: true,
        });
        this._scrollView.add_child(this._resultsBox);

        // Assemble card
        this._card.add_child(searchBar);
        this._card.add_child(separator);
        this._card.add_child(this._scrollView);

        // Assemble container (backdrop behind card)
        this._container.add_child(this._backdrop);
        this._container.add_child(this._card);
    }

    _destroyUi() {
        if (this._container) {
            this._container.destroy_all_children();
            if (this._container.get_parent())
                Main.uiGroup.remove_child(this._container);
            this._container = null;
        }
        this._card = null;
        this._backdrop = null;
        this._entry = null;
        this._scrollView = null;
        this._resultsBox = null;
        this._resultRows = [];
    }

    // ── Show / hide ─────────────────────────────────────────────────

    _show() {
        if (this._visible) return;
        if (!this._container) return;

        // Size backdrop to cover all monitors
        let bounds = this._monitorBounds();
        this._backdrop.set_position(bounds.x, bounds.y);
        this._backdrop.set_size(bounds.w, bounds.h);

        // Clear
        this._entry.set_text('');
        this._resultsBox.destroy_all_children();
        this._resultRows = [];
        this._selectedIndex = -1;
        this._scrollView.set_height(0);

        // Add to stage
        Main.uiGroup.add_child(this._container);

        // Center card
        this._centerCard();

        // Focus
        this._entry.grab_key_focus();

        // Fade in
        this._container.set_opacity(0);
        this._container.ease({
            opacity: 255,
            duration: 180,
            mode: Clutter.AnimationMode.EASE_OUT_QUAD,
        });

        this._visible = true;
        _DBG('shown');
    }

    _hide(immediate = false) {
        if (!this._visible) return;
        this._visible = false;

        // Cancel pending search
        if (this._debounceId) {
            GLib.source_remove(this._debounceId);
            this._debounceId = 0;
        }

        if (immediate || !this._container) {
            if (this._container && this._container.get_parent())
                Main.uiGroup.remove_child(this._container);
            return;
        }

        // Fade out then remove
        this._container.ease({
            opacity: 0,
            duration: 130,
            mode: Clutter.AnimationMode.EASE_OUT_QUAD,
            onComplete: () => {
                if (this._container && this._container.get_parent())
                    Main.uiGroup.remove_child(this._container);
            },
        });
        _DBG('hidden');
    }

    _toggle() {
        if (this._visible) this._hide();
        else this._show();
    }

    _centerCard() {
        let monitor = Main.layoutManager.primaryMonitor;
        // Need to get card height after layout
        GLib.idle_add(GLib.PRIORITY_DEFAULT, () => {
            if (!this._visible || !this._card) return GLib.SOURCE_REMOVE;
            let cardH = this._card.height || 120;
            let x = monitor.x + Math.floor((monitor.width - CARD_WIDTH) / 2);
            let y = monitor.y + Math.floor((monitor.height - cardH) / 2)
                    - Math.floor(monitor.height * 0.12);
            this._card.set_position(x, y);
            _DBG('centered at ' + x + ',' + y + ' (cardH=' + cardH + ')');
            return GLib.SOURCE_REMOVE;
        });
    }

    _monitorBounds() {
        let n = global.display.get_n_monitors();
        let minX = 0, minY = 0, maxX = 0, maxY = 0;
        for (let i = 0; i < n; i++) {
            let g = global.display.get_monitor_geometry(i);
            minX = Math.min(minX, g.x);
            minY = Math.min(minY, g.y);
            maxX = Math.max(maxX, g.x + g.width);
            maxY = Math.max(maxY, g.y + g.height);
        }
        return { x: minX, y: minY, w: maxX - minX, h: maxY - minY };
    }

    // ── Search ──────────────────────────────────────────────────────

    _onTextChanged() {
        if (this._debounceId) {
            GLib.source_remove(this._debounceId);
            this._debounceId = 0;
        }
        let query = this._entry.get_text();
        if (query.trim().length === 0) {
            this._clearResults();
            return;
        }
        this._debounceId = GLib.timeout_add(GLib.PRIORITY_DEFAULT, DEBOUNCE_MS, () => {
            this._debounceId = 0;
            this._doSearch(query);
            return GLib.SOURCE_REMOVE;
        });
    }

    _doSearch(query) {
        if (!this._binaryPath) {
            _DBG('no binary, cannot search');
            return;
        }
        try {
            let proc = new Gio.Subprocess({
                argv: [this._binaryPath, '--search', query],
                flags: Gio.SubprocessFlags.STDOUT_PIPE,
            });
            proc.init(null);
            proc.communicate_utf8_async(null, null, (p, res) => {
                try {
                    let [, stdout] = p.communicate_utf8_finish(res);
                    let result = JSON.parse(stdout);
                    this._displayResults(result.items || []);
                } catch (e) {
                    _DBG('search error: ' + e.message);
                }
            });
        } catch (e) {
            _DBG('spawn error: ' + e.message);
        }
    }

    _clearResults() {
        this._resultsBox.destroy_all_children();
        this._resultRows = [];
        this._selectedIndex = -1;
        this._scrollView.set_height(0);
        if (this._visible) this._centerCard();
    }

    _displayResults(items) {
        this._resultsBox.destroy_all_children();
        this._resultRows = [];
        this._selectedIndex = -1;

        for (let i = 0; i < items.length; i++) {
            let item = items[i];
            let row = new St.BoxLayout({
                style_class: 'spotlight-result-row',
                reactive: true,
                can_focus: true,
            });

            let icon = new St.Icon({
                icon_name: item.icon || 'text-x-generic',
                icon_size: 28,
                y_align: Clutter.ActorAlign.CENTER,
            });

            let textBox = new St.BoxLayout({
                vertical: true,
                y_align: Clutter.ActorAlign.CENTER,
                x_expand: true,
            });
            let title = new St.Label({
                text: item.title,
                style_class: 'spotlight-result-title',
            });
            let subtitle = new St.Label({
                text: item.subtitle,
                style_class: 'spotlight-result-subtitle',
            });
            textBox.add_child(title);
            textBox.add_child(subtitle);

            row.add_child(icon);
            row.add_child(textBox);

            let idx = i;
            row.connect('button-press-event', () => {
                this._activateResult(idx);
                return Clutter.EVENT_STOP;
            });
            row.connect('enter-event', () => {
                this._selectIndex(idx);
                return Clutter.EVENT_STOP;
            });

            this._resultsBox.add_child(row);
            this._resultRows.push({ row, item });
        }

        // Adjust scroll height
        let h = items.length > 0
            ? Math.min(items.length * ROW_HEIGHT + 12, MAX_RESULTS_HEIGHT)
            : 0;
        this._scrollView.set_height(h);

        // Re-center card with new height
        if (this._visible) this._centerCard();
        _DBG('displayed ' + items.length + ' results');
    }

    // ── Selection / activation ─────────────────────────────────────

    _selectIndex(index) {
        if (index < 0 || index >= this._resultRows.length) return;
        this._selectedIndex = index;
        this._updateSelection();

        // Scroll selected row into view
        let row = this._resultRows[index].row;
        let adjust = this._scrollView.vscroll.adjustment;
        let rowY = index * ROW_HEIGHT;
        let viewStart = adjust.value;
        let viewEnd = viewStart + adjust.page_size;
        if (rowY < viewStart)
            adjust.value = rowY;
        else if (rowY + ROW_HEIGHT > viewEnd)
            adjust.value = rowY + ROW_HEIGHT - adjust.page_size;
    }

    _updateSelection() {
        for (let i = 0; i < this._resultRows.length; i++) {
            if (i === this._selectedIndex)
                this._resultRows[i].row.add_style_class_name('selected');
            else
                this._resultRows[i].row.remove_style_class_name('selected');
        }
    }

    _activateResult(index) {
        if (index < 0 || index >= this._resultRows.length) return;
        let item = this._resultRows[index].item;
        _DBG('activate: ' + item.action_type + ' ' + item.action_data);
        try {
            switch (item.action_type) {
                case 'launch_app':
                    Gio.Subprocess.new(['gtk-launch', item.action_data],
                        Gio.SubprocessFlags.NONE);
                    break;
                case 'open_file':
                    Gio.Subprocess.new(['xdg-open', item.action_data],
                        Gio.SubprocessFlags.NONE);
                    break;
                case 'copy':
                    let clip = St.Clipboard.get_default();
                    let clipType = (St.ClipboardType && St.ClipboardType.CLIPBOARD) ?? 0;
                    clip.set_text(clipType, item.action_data);
                    break;
            }
        } catch (e) {
            _DBG('activate error: ' + e.message);
        }
        this._hide();
    }

    // ── Keyboard ────────────────────────────────────────────────────

    _onKeyPress(event) {
        let keyval = event.get_key_symbol();
        switch (keyval) {
            case Clutter.KEY_Escape:
                this._hide();
                return Clutter.EVENT_STOP;

            case Clutter.KEY_Down:
            case Clutter.KEY_Tab:
                if (this._resultRows.length > 0) {
                    let next = this._selectedIndex + 1;
                    if (next >= this._resultRows.length) next = 0;
                    this._selectIndex(next);
                }
                return Clutter.EVENT_STOP;

            case Clutter.KEY_Up:
                if (this._selectedIndex > 0) {
                    this._selectIndex(this._selectedIndex - 1);
                } else if (this._selectedIndex === 0) {
                    this._selectedIndex = -1;
                    this._updateSelection();
                    this._entry.grab_key_focus();
                }
                return Clutter.EVENT_STOP;

            case Clutter.KEY_Return:
            case Clutter.KEY_KP_Enter:
                if (this._selectedIndex >= 0) {
                    this._activateResult(this._selectedIndex);
                } else if (this._resultRows.length > 0) {
                    this._activateResult(0);
                }
                return Clutter.EVENT_STOP;
        }
        return Clutter.EVENT_PROPAGATE;
    }

    // ── D-Bus toggle interface ──────────────────────────────────────

    _registerDbus() {
        const ifaceXml = `
<node>
  <interface name='dev.SpotlightFiles'>
    <method name='Toggle'/>
    <method name='Show'/>
    <method name='Hide'/>
  </interface>
</node>`;

        let self = this;
        let handler = {
            Toggle: () => self._toggle(),
            Show: () => self._show(),
            Hide: () => self._hide(),
        };

        try {
            this._dbus = Gio.DBusExportedObject.wrapJSObject(ifaceXml, handler);
            this._dbusId = Gio.DBus.session.register_object(
                '/dev/SpotlightFiles', this._dbus);
            _DBG('dbus registered (id=' + this._dbusId + ')');
        } catch (e) {
            _DBG('dbus error: ' + e.message);
        }
    }

    _unregisterDbus() {
        if (this._dbusId > 0) {
            try {
                Gio.DBus.session.unregister_object(this._dbusId);
            } catch (e) {}
            this._dbusId = 0;
        }
    }
}
