// Tray icon: brings hidden windows back, starts sessions, starts reframe with the system, installs updates,
// and is the only way to really quit.
import { Menu, Tray, nativeImage } from 'electron';

export interface TrayWindowItem {
  id: string;
  label: string;
  visible: boolean;
}

/** The menu's texts, read on every refresh (the language can change while reframe runs). */
export interface TrayLabels {
  tooltip: string;
  newSession: string;
  newWindow: string;
  autostart: string;
  about: string;
  quit: string;
}

export interface TrayActions {
  labels: () => TrayLabels;
  windows: () => TrayWindowItem[];
  toggleWindow: (id: string, visible: boolean) => void;
  showPrimary: () => void;
  newSession: () => void;
  newWindow: () => void;
  autostart: () => boolean;
  setAutostart: (enabled: boolean) => void;
  /** "Restart to update" once an update is downloaded. */
  update: () => { label: string; install: () => void } | null;
  about: () => void;
  quit: () => void;
}

export function createTray(icon: string, icon2x: string, actions: TrayActions): { tray: Tray; refresh: () => void } {
  const image = nativeImage.createFromPath(icon);
  image.addRepresentation({ scaleFactor: 2, buffer: nativeImage.createFromPath(icon2x).toPNG() });
  const tray = new Tray(image);

  const refresh = () => {
    const labels = actions.labels();
    const update = actions.update();
    tray.setToolTip(labels.tooltip);
    tray.setContextMenu(
      Menu.buildFromTemplate([
        ...actions.windows().map((w) => ({
          label: w.label,
          type: 'checkbox' as const,
          checked: w.visible,
          click: () => actions.toggleWindow(w.id, !w.visible),
        })),
        { type: 'separator' },
        { label: labels.newSession, click: actions.newSession },
        { label: labels.newWindow, click: actions.newWindow },
        { type: 'separator' },
        {
          label: labels.autostart,
          type: 'checkbox',
          checked: actions.autostart(),
          click: (item) => {
            actions.setAutostart(item.checked);
            refresh();
          },
        },
        ...(update ? [{ label: update.label, click: update.install }] : []),
        { label: labels.about, click: actions.about },
        { label: labels.quit, click: actions.quit },
      ]),
    );
  };
  refresh();
  tray.on('click', actions.showPrimary);
  return { tray, refresh };
}
