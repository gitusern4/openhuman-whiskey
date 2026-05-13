import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

// Static import is fine — the mocks above are hoisted by vitest.
import WindowsMascotApp from './WindowsMascotApp';

// --- Tauri API mocks ---------------------------------------------------------
// Mock the two @tauri-apps modules WindowsMascotApp imports from. We hold
// the mock fns in module-scope vars so each test can assert against them
// and override return values per-case.
//
// Explicit generic signatures on each `vi.fn<>()` because TypeScript would
// otherwise infer zero-arg shapes from the default factories and reject the
// `mockImplementation(async (cmd) => ...)` overrides used by the
// error-handling tests below.
type UnlistenFn = () => void;
type InvokeFn = (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;
type IsTauriFn = () => boolean;
type StartDraggingFn = () => Promise<void>;
type ListenFn = (event: string, handler: (event: unknown) => void) => Promise<UnlistenFn>;

const mockInvoke = vi.fn<InvokeFn>(async () => undefined);
let mockIsTauri = vi.fn<IsTauriFn>(() => true);
const mockStartDragging = vi.fn<StartDraggingFn>(async () => undefined);
const mockListen = vi.fn<ListenFn>(async () => () => undefined);

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (cmd: string, args?: Record<string, unknown>) => mockInvoke(cmd, args),
  isTauri: () => mockIsTauri(),
}));

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({
    listen: (event: string, handler: (event: unknown) => void) => mockListen(event, handler),
    startDragging: () => mockStartDragging(),
  }),
}));

// Stub the heavy YellowMascot SVG so the unit test stays tight + the
// component-under-test is the only thing exercised. The real
// YellowMascot pulls in remotion / SVG layers that aren't useful here.
vi.mock('../features/human/Mascot', () => ({
  YellowMascot: () => <div data-testid="yellow-mascot-stub" />,
}));

const ROOT_TESTID = 'windows-mascot-root';

function fireDragSequence(start: { x: number; y: number }, end: { x: number; y: number }) {
  const root = screen.getByTestId(ROOT_TESTID);
  fireEvent.pointerDown(root, { clientX: start.x, clientY: start.y });
  fireEvent.pointerMove(root, { clientX: end.x, clientY: end.y });
  fireEvent.pointerUp(root, { clientX: end.x, clientY: end.y });
}

describe('WindowsMascotApp', () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    mockInvoke.mockImplementation(async () => undefined);
    mockIsTauri = vi.fn(() => true);
    mockStartDragging.mockReset();
    mockStartDragging.mockImplementation(async () => undefined);
    mockListen.mockReset();
    mockListen.mockImplementation(async () => () => undefined);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('renders the YellowMascot inside the draggable root', () => {
    render(<WindowsMascotApp />);
    expect(screen.getByTestId(ROOT_TESTID)).toBeInTheDocument();
    expect(screen.getByTestId('yellow-mascot-stub')).toBeInTheDocument();
    // The root carries the data-face hint so external CSS / debug
    // tooling can branch on the agent's current state.
    expect(screen.getByTestId(ROOT_TESTID)).toHaveAttribute('data-face', 'idle');
  });

  it('subscribes to tauri://move on mount and unsubscribes on unmount', async () => {
    const unlisten = vi.fn();
    mockListen.mockImplementationOnce(async () => unlisten);

    const { unmount } = render(<WindowsMascotApp />);
    await waitFor(() =>
      expect(mockListen).toHaveBeenCalledWith('tauri://move', expect.any(Function))
    );

    unmount();
    // listen() resolves before unmount fires the cleanup, but the
    // useEffect-cleanup sets a closure-captured handle so the unlisten
    // fn fires on the next tick. Wait for it.
    await waitFor(() => expect(unlisten).toHaveBeenCalled());
  });

  it('treats a sub-threshold pointer move as a click and calls activate_main_window', () => {
    render(<WindowsMascotApp />);
    // Move 2px — under the 4px drag threshold.
    fireDragSequence({ x: 50, y: 50 }, { x: 51, y: 51 });

    expect(mockStartDragging).not.toHaveBeenCalled();
    // The wrapper forwards (cmd, args) to mockInvoke; args is `undefined`
    // when WindowsMascotApp calls `invoke('activate_main_window')` with
    // no payload. Match the literal call shape, not the user-facing API.
    expect(mockInvoke).toHaveBeenCalledWith('activate_main_window', undefined);
  });

  it('treats a supra-threshold pointer move as a drag and calls startDragging', () => {
    render(<WindowsMascotApp />);
    // Move 10px diagonally (~14px Euclidean) — over the 4px threshold.
    fireDragSequence({ x: 50, y: 50 }, { x: 60, y: 60 });

    expect(mockStartDragging).toHaveBeenCalledTimes(1);
    // After a drag, the pointerUp should NOT also fire the click handler.
    expect(mockInvoke).not.toHaveBeenCalledWith('activate_main_window');
  });

  it('persists position when the tauri://move event fires', async () => {
    let capturedHandler: ((event: unknown) => void) | undefined;
    mockListen.mockImplementationOnce(async (_evt: unknown, handler: (event: unknown) => void) => {
      capturedHandler = handler;
      return () => undefined;
    });

    render(<WindowsMascotApp />);
    await waitFor(() => expect(capturedHandler).toBeDefined());

    capturedHandler?.({ payload: { x: 1, y: 2 } });

    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith('mascot_window_save_position', undefined)
    );
  });

  it('skips Tauri calls entirely when not in a Tauri runtime', () => {
    mockIsTauri = vi.fn(() => false);

    render(<WindowsMascotApp />);
    fireDragSequence({ x: 0, y: 0 }, { x: 100, y: 100 });

    expect(mockListen).not.toHaveBeenCalled();
    expect(mockStartDragging).not.toHaveBeenCalled();
    expect(mockInvoke).not.toHaveBeenCalled();
  });

  it('swallows startDragging errors without crashing', async () => {
    mockStartDragging.mockRejectedValueOnce(new Error('drag boom'));
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => undefined);

    render(<WindowsMascotApp />);
    fireDragSequence({ x: 0, y: 0 }, { x: 100, y: 100 });

    await waitFor(() =>
      expect(warnSpy).toHaveBeenCalledWith('[mascot-win] startDragging failed', expect.any(Error))
    );
  });

  it('swallows activate_main_window errors without crashing', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'activate_main_window') throw new Error('pop boom');
      return undefined;
    });
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => undefined);

    render(<WindowsMascotApp />);
    fireDragSequence({ x: 0, y: 0 }, { x: 1, y: 1 });

    await waitFor(() =>
      expect(warnSpy).toHaveBeenCalledWith(
        '[mascot-win] activate_main_window failed',
        expect.any(Error)
      )
    );
  });
});
