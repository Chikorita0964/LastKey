#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
#include <shellapi.h>
#include <shlobj.h>

#include <optional>
#include <string>

#include "resource.h"
#include "SocdState.h"

namespace {

using lastkey::EventDisposition;
using lastkey::Key;
using lastkey::KeyAction;
using lastkey::SocdState;
using lastkey::kKeyCount;
using lastkey::ToIndex;

struct KeySpec {
    WORD scanCode;
    bool extended;
};

// Default: W/S and A/D.
constexpr KeySpec kKeys[kKeyCount] = {
    {0x11, false}, // W
    {0x1F, false}, // S
    {0x1E, false}, // A
    {0x20, false}, // D
};

// To use the arrow keys instead, replace kKeys above with:
// constexpr KeySpec kKeys[kKeyCount] = {
//     {0x48, true}, // Up
//     {0x50, true}, // Down
//     {0x4B, true}, // Left
//     {0x4D, true}, // Right
// };

constexpr ULONG_PTR kInjectionTag = static_cast<ULONG_PTR>(0x4C4153544B455931ULL); // "LASTKEY1"
constexpr UINT kTrayCallbackMessage = WM_APP + 1;
constexpr UINT kCreateDesktopShortcutCommand = 1;
constexpr UINT kExitCommand = 2;
constexpr wchar_t kWindowClassName[] = L"LastKeyTrayWindow";
constexpr wchar_t kTaskbarCreatedName[] = L"TaskbarCreated";
constexpr wchar_t kInstanceMutexName[] = L"Local\\LastKey-Instance";

SocdState g_socd;
HHOOK g_hook = nullptr;
HICON g_trayIcon = nullptr;
UINT g_taskbarCreatedMessage = 0;

void ShowError(const wchar_t* message) {
    MessageBoxW(nullptr, message, L"LastKey", MB_OK | MB_ICONERROR);
}

void ShowInformation(const wchar_t* message) {
    MessageBoxW(nullptr, message, L"LastKey", MB_OK | MB_ICONINFORMATION);
}

std::optional<std::wstring> GetCurrentExecutablePath() {
    std::wstring path(MAX_PATH, L'\0');

    while (true) {
        const DWORD pathLength = GetModuleFileNameW(nullptr, path.data(),
                                                     static_cast<DWORD>(path.size()));
        if (pathLength == 0) return std::nullopt;
        if (pathLength < path.size()) {
            path.resize(pathLength);
            return path;
        }

        path.resize(path.size() * 2);
    }
}

std::optional<std::wstring> GetDesktopShortcutPath() {
    PWSTR desktopPath = nullptr;
    const HRESULT result =
        SHGetKnownFolderPath(FOLDERID_Desktop, KF_FLAG_DEFAULT, nullptr, &desktopPath);
    if (FAILED(result) || !desktopPath) {
        CoTaskMemFree(desktopPath);
        return std::nullopt;
    }

    const std::wstring shortcutPath = std::wstring(desktopPath) + L"\\LastKey.lnk";
    CoTaskMemFree(desktopPath);
    return shortcutPath;
}

bool CreateDesktopShortcut() {
    const std::optional<std::wstring> executablePath = GetCurrentExecutablePath();
    if (!executablePath) return false;

    const HRESULT initializeResult = CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);
    if (FAILED(initializeResult)) return false;

    const std::optional<std::wstring> shortcutPath = GetDesktopShortcutPath();
    if (!shortcutPath) {
        CoUninitialize();
        return false;
    }

    IShellLinkW* shortcut = nullptr;
    const HRESULT createResult = CoCreateInstance(CLSID_ShellLink, nullptr, CLSCTX_INPROC_SERVER,
                                                  IID_PPV_ARGS(&shortcut));
    if (FAILED(createResult)) {
        CoUninitialize();
        return false;
    }

    HRESULT result = shortcut->SetPath(executablePath->c_str());

    IPersistFile* persistFile = nullptr;
    if (SUCCEEDED(result)) result = shortcut->QueryInterface(IID_PPV_ARGS(&persistFile));
    if (SUCCEEDED(result)) {
        result = persistFile->Save(shortcutPath->c_str(), TRUE);
        persistFile->Release();
    }

    shortcut->Release();
    CoUninitialize();
    return SUCCEEDED(result);
}

const KeySpec& Spec(Key key) {
    return kKeys[ToIndex(key)];
}

std::optional<Key> FindKey(const KBDLLHOOKSTRUCT& key) {
    const bool extended = (key.flags & LLKHF_EXTENDED) != 0;
    for (std::size_t index = 0; index < kKeyCount; ++index) {
        if (key.scanCode == kKeys[index].scanCode && extended == kKeys[index].extended)
            return static_cast<Key>(index);
    }
    return std::nullopt;
}

std::optional<KeyAction> FindAction(WPARAM message) {
    if (message == WM_KEYDOWN || message == WM_SYSKEYDOWN) return KeyAction::Down;
    if (message == WM_KEYUP || message == WM_SYSKEYUP) return KeyAction::Up;
    return std::nullopt;
}

bool EmitKeyEvent(Key key, KeyAction action) {
    const KeySpec& spec = Spec(key);
    INPUT input{};
    input.type = INPUT_KEYBOARD;
    input.ki.wScan = spec.scanCode;
    input.ki.dwFlags = KEYEVENTF_SCANCODE |
                       (spec.extended ? KEYEVENTF_EXTENDEDKEY : 0) |
                       (action == KeyAction::Up ? KEYEVENTF_KEYUP : 0);
    input.ki.dwExtraInfo = kInjectionTag;
    return SendInput(1, &input, sizeof(input)) == 1;
}

LRESULT CALLBACK KeyboardProc(int code, WPARAM message, LPARAM lParam) {
    if (code != HC_ACTION) return CallNextHookEx(g_hook, code, message, lParam);
    const auto& event = *reinterpret_cast<const KBDLLHOOKSTRUCT*>(lParam);

    // Events we generated must reach the target application, but must not mutate state.
    if ((event.flags & LLKHF_INJECTED) != 0 && event.dwExtraInfo == kInjectionTag)
        return CallNextHookEx(g_hook, code, message, lParam);

    const std::optional<Key> key = FindKey(event);
    if (!key) return CallNextHookEx(g_hook, code, message, lParam);

    const std::optional<KeyAction> action = FindAction(message);
    if (!action)
        return CallNextHookEx(g_hook, code, message, lParam);

    if (g_socd.Process(*key, *action, EmitKeyEvent) == EventDisposition::PassThrough)
        return CallNextHookEx(g_hook, code, message, lParam);

    // Consume originals unless one safely replaced a failed SendInput event.
    return 1;
}

bool UpdateTrayIcon(HWND window, DWORD operation) {
    NOTIFYICONDATAW icon{};
    icon.cbSize = sizeof(icon);
    icon.hWnd = window;
    icon.uID = IDI_LASTKEY;
    icon.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    icon.uCallbackMessage = kTrayCallbackMessage;
    icon.hIcon = g_trayIcon;
    lstrcpynW(icon.szTip, L"LastKey - W/S, A/D", ARRAYSIZE(icon.szTip));
    return Shell_NotifyIconW(operation, &icon) != FALSE;
}

bool AddTrayIcon(HWND window) {
    return UpdateTrayIcon(window, NIM_ADD);
}

bool ModifyTrayIcon(HWND window) {
    return UpdateTrayIcon(window, NIM_MODIFY);
}

void RemoveTrayIcon(HWND window) {
    NOTIFYICONDATAW icon{};
    icon.cbSize = sizeof(icon);
    icon.hWnd = window;
    icon.uID = IDI_LASTKEY;
    Shell_NotifyIconW(NIM_DELETE, &icon);
}

bool RestoreTrayIcon(HWND window) {
    // Explorer removes notification icons before broadcasting TaskbarCreated.
    // Some broadcasts leave the icon intact, so modify it if adding fails.
    return AddTrayIcon(window) || ModifyTrayIcon(window);
}

void ShowTrayMenu(HWND window) {
    POINT point{};
    GetCursorPos(&point);

    HMENU menu = CreatePopupMenu();
    if (!menu) return;

    AppendMenuW(menu, MF_STRING, kCreateDesktopShortcutCommand, L"Create desktop shortcut");
    AppendMenuW(menu, MF_SEPARATOR, 0, nullptr);
    AppendMenuW(menu, MF_STRING, kExitCommand, L"Exit");
    SetForegroundWindow(window);
    TrackPopupMenu(menu, TPM_RIGHTBUTTON, point.x, point.y, 0, window, nullptr);
    DestroyMenu(menu);
    PostMessageW(window, WM_NULL, 0, 0);
}

LRESULT CALLBACK WindowProc(HWND window, UINT message, WPARAM wParam, LPARAM lParam) {
    if (g_taskbarCreatedMessage != 0 && message == g_taskbarCreatedMessage) {
        if (!RestoreTrayIcon(window)) {
            ShowError(L"Unable to restore the tray icon. LastKey will exit.");
            DestroyWindow(window);
        }
        return 0;
    }

    switch (message) {
    case kTrayCallbackMessage:
        if (lParam == WM_RBUTTONUP || lParam == WM_CONTEXTMENU) ShowTrayMenu(window);
        return 0;
    case WM_COMMAND:
        switch (LOWORD(wParam)) {
        case kCreateDesktopShortcutCommand:
            if (!CreateDesktopShortcut()) ShowError(L"Unable to create the desktop shortcut.");
            return 0;
        case kExitCommand:
            DestroyWindow(window);
            return 0;
        }
        return 0;
    case WM_DESTROY: {
        RemoveTrayIcon(window);
        if (g_trayIcon) {
            DestroyIcon(g_trayIcon);
            g_trayIcon = nullptr;
        }
        PostQuitMessage(0);
        return 0;
    }
    default:
        return DefWindowProcW(window, message, wParam, lParam);
    }
}
} // namespace

int WINAPI wWinMain(HINSTANCE instance, HINSTANCE, PWSTR, int) {
    // Keeps a handle open while this instance is running.
    const HANDLE instanceMutex = CreateMutexW(nullptr, FALSE, kInstanceMutexName);
    if (!instanceMutex) {
        ShowError(L"Unable to start LastKey.");
        return 1;
    }
    if (GetLastError() == ERROR_ALREADY_EXISTS) {
        CloseHandle(instanceMutex);
        ShowInformation(L"LastKey is already running.");
        return 0;
    }

    g_taskbarCreatedMessage = RegisterWindowMessageW(kTaskbarCreatedName);
    if (g_taskbarCreatedMessage == 0) {
        CloseHandle(instanceMutex);
        ShowError(L"Unable to initialize LastKey.");
        return 1;
    }

    WNDCLASSW windowClass{};
    windowClass.hInstance = instance;
    windowClass.lpszClassName = kWindowClassName;
    windowClass.lpfnWndProc = WindowProc;
    if (!RegisterClassW(&windowClass)) {
        CloseHandle(instanceMutex);
        ShowError(L"Unable to initialize LastKey.");
        return 1;
    }

    HWND window = CreateWindowExW(0, kWindowClassName, L"LastKey", 0,
                                  0, 0, 0, 0, nullptr, nullptr, instance, nullptr);
    if (!window) {
        CloseHandle(instanceMutex);
        ShowError(L"Unable to initialize LastKey.");
        return 1;
    }

    g_trayIcon = static_cast<HICON>(LoadImageW(instance, MAKEINTRESOURCEW(IDI_LASTKEY),
                                                IMAGE_ICON, 0, 0, LR_DEFAULTSIZE));
    if (!g_trayIcon || !AddTrayIcon(window)) {
        DestroyWindow(window);
        CloseHandle(instanceMutex);
        ShowError(L"Unable to add the tray icon.");
        return 1;
    }

    g_hook = SetWindowsHookExW(WH_KEYBOARD_LL, KeyboardProc, instance, 0);
    if (!g_hook) {
        DestroyWindow(window);
        CloseHandle(instanceMutex);
        ShowError(L"Unable to install the keyboard hook.");
        return 1;
    }

    MSG message{};
    int exitCode = 0;
    while (true) {
        const int result = GetMessageW(&message, nullptr, 0, 0);
        if (result == -1) {
            exitCode = 1;
            break;
        }
        if (result == 0) break;
        TranslateMessage(&message);
        DispatchMessageW(&message);
    }

    g_socd.ReleaseAll(EmitKeyEvent);
    UnhookWindowsHookEx(g_hook);
    CloseHandle(instanceMutex);
    return exitCode;
}
