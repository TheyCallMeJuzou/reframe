// Tray icon: brings hidden windows back, starts sessions, and is the only way to really quit.
import { Menu, Tray, nativeImage } from 'electron';

export interface TrayWindowItem {
  id: string;
  label: string;
  visible: boolean;
}

export interface TrayActions {
  tooltip: string;
  newSessionLabel: string;
  quitLabel: string;
  windows: () => TrayWindowItem[];
  toggleWindow: (id: string, visible: boolean) => void;
  showPrimary: () => void;
  newSession: () => void;
  quit: () => void;
}

export function createTray(icon: string, icon2x: string, actions: TrayActions): { tray: Tray; refresh: () => void } {
  const image = nativeImage.createFromPath(icon);
  image.addRepresentation({ scaleFactor: 2, buffer: nativeImage.createFromPath(icon2x).toPNG() });
  const tray = new Tray(image);
  tray.setToolTip(actions.tooltip);

  const refresh = () => {
    tray.setContextMenu(
      Menu.buildFromTemplate([
        ...actions.windows().map((w) => ({
          label: w.label,
          type: 'checkbox' as const,
          checked: w.visible,
          click: () => actions.toggleWindow(w.id, !w.visible),
        })),
        { type: 'separator' },
        { label: actions.newSessionLabel, click: actions.newSession },
        { type: 'separator' },
        { label: actions.quitLabel, click: actions.quit },
      ]),
    );
  };
  refresh();
  tray.on('click', actions.showPrimary);
  return { tray, refresh };
}
