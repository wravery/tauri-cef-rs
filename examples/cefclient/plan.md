# Cefclient C++ to Rust Porting Plan

## Current State Analysis

### What's Already Ported (Complete or Partial)

| Component | Status | Files |
|-----------|--------|-------|
| **Main entry** | Basic | `main.rs`, `lib.rs` |
| **CEF loading** | Complete | `mod.rs` (`load_cef()`, `run_main()`) |
| **SimpleApp** | Partial | `simple_app.rs` (BrowserProcessHandler only) |
| **SimpleHandler** | Partial | `simple_handler/mod.rs` (Display, LifeSpan, Load handlers) |
| **Custom schemes** | Complete | `common.rs` |
| **Resource IDs** | Complete | `resources.rs` |
| **Platform title** | Partial | `linux.rs`, `win.rs` (title change only) |
| **OsrRenderer** | Partial | `osr_renderer.rs` (settings only) |

### What's Missing (Major Components)

The C++ cefclient has **100+ source files**. The Rust port currently has roughly **25 files**. Here's what needs to be ported:

---

## Phase 1: Core Architecture

### 1.1 MainContext (Application State)
**C++ files:** `main_context.h/cc`, `main_context_impl.h/cc`, `main_context_impl_posix.cc`, `main_context_impl_win.cc`

**Tasks:**
- [ ] Implement `MainContext` trait/interface for global state management
- [ ] Command-line argument processing and parsing
- [ ] CEF initialization/shutdown orchestration
- [ ] Platform-specific main context setup (POSIX vs Windows)

### 1.2 RootWindowManager (Window Management)
**C++ files:** `root_window_manager.h/cc`, `root_window.h/cc`, `root_window_create.cc`

**Tasks:**
- [ ] Implement `RootWindowManager` for tracking browser windows
- [ ] Window creation/destruction lifecycle
- [ ] Popup window management
- [ ] Window bounds saving/restoring (preferences)

### 1.3 ClientHandler (Main Client Implementation)
**C++ files:** `client_handler.h/cc` (**56KB** - the biggest file!), `client_handler_std.h/cc`, `client_handler_osr.h/cc`

**Tasks:**
- [ ] Implement the full `Client` handler with all delegate callbacks
- [ ] **Download handling** (`DownloadHandler`) - file downloads, progress
- [ ] **Keyboard handling** (`KeyboardHandler`) - native key events
- [ ] **Display extensions** - context menu, splash screen
- [ ] **LifeSpan extensions** - dialog handling, before unload
- [ ] **Load extensions** - status message, console message, unresponsive script
- [ ] **Render handler** (`RenderHandler`) for OSR support
- [ ] **Render process handler** (`RenderProcessHandler`)
- [ ] **Geolocation handler**
- [ ] **Audio handler**

---

## Phase 2: Platform-Specific Window Implementations

### 2.1 Linux (GTK)
**C++ files:** `cefclient_gtk.cc`, `root_window_gtk.h/cc`, `browser_window_std_gtk.h/cc`, `browser_window_osr_gtk.h/cc`, `dialog_handler_gtk.h/cc`, `print_handler_gtk.h/cc`, `util_gtk.h/cc`

**Tasks:**
- [ ] GTK window creation and integration
- [ ] GTK menu bar implementation
- [ ] GTK dialog handlers (file, color, font dialogs)
- [ ] GTK print dialog integration
- [ ] Standard browser window (native GTK widgets)
- [ ] OSR browser window (GTK drawing surface)
- [ ] X11/Xkb input handling

### 2.2 Windows (Win32)
**C++ files:** `cefclient_win.cc`, `root_window_win.h/cc`, `browser_window_std_win.h/cc`, `browser_window_osr_win.h/cc`, `osr_window_win.h/cc`, `osr_d3d11_win.h/cc`, `osr_render_handler_win*.h/cc`, `osr_ime_handler_win.h/cc`, `osr_dragdrop_win.h/cc`, `temp_window_win.h/cc`, `resource_util_win_idmap.cc`

**Tasks:**
- [ ] Win32 window creation and message handling
- [ ] D3D11 OSR rendering backend
- [ ] OpenGL OSR rendering backend
- [ ] IME (Input Method Editor) handler for CJK input
- [ ] Drag-and-drop support
- [ ] Window icon/resource loading
- [ ] Popup window management

### 2.3 macOS (Cocoa)
**C++ files:** `cefclient_mac.mm`, `root_window_mac.h`, `root_window_mac.mm`, `browser_window_std_mac.h/mm`, `browser_window_osr_mac.h`, `views_window_mac.mm`, `text_input_client_osr_mac.h/mm`, `temp_window_mac.h/mm`, `util_mac.h/mm`

**Tasks:**
- [ ] Cocoa window/NSWindow integration
- [ ] MainMenu.xib menu bar
- [ ] NSTextInputClient for OSR text input
- [ ] AppleScript support
- [ ] Application delegate setup

---

## Phase 3: Views Framework (Cross-Platform UI)

### 3.1 Views Window System
**C++ files:** `root_window_views.h/cc`, `views_window.h/cc`, `views_window_mac.mm`, `views_overlay_browser.h/cc`, `views_overlay_controls.h/cc`, `views_menu_bar.h/cc`, `views_style.h/cc`

**Tasks:**
- [ ] CEF Views framework integration
- [ ] Cross-platform menu bar
- [ ] Browser overlay (toolbar with nav buttons)
- [ ] Control overlay (zoom, audio mute, find)
- [ ] Views-style theming (Alloy vs Chrome)
- [ ] Window state management via Views

---

## Phase 4: Test Framework

### 4.1 Test Runner Infrastructure
**C++ files:** `test_runner.h/cc` (**29KB**)

**Tasks:**
- [ ] Test menu generation from command-line metadata
- [ ] Test registry and execution framework
- [ ] Test result reporting
- [ ] JavaScript test injection

### 4.2 Individual Tests (Port All That Are Ported)
**Already has partial Rust files:**

| Test | C++ Status | Rust Status |
|------|------------|-------------|
| BinaryTransferTest | `binary_transfer_test.h/cc` | `binary_transfer_test.rs` |
| BindingTest | `binding_test.h/cc` | `binding_test.rs` |
| ConfigTest | `config_test.h/cc` | `config_test.rs` |
| DialogTest | `dialog_test.h/cc` | `dialog_test.rs` |
| HangTest | `hang_test.h/cc` | `hang_test.rs` |
| MediaRouterTest | `media_router_test.h/cc` | `media_router_test.rs` |
| PreferencesTest | `preferences_test.h/cc` | `preferences_test.rs` |
| ResponseFilterTest | `response_filter_test.h/cc` | `response_filter_test.rs` |
| ServerTest | `server_test.h/cc` | `server_test.rs` |
| TaskManagerTest | `task_manager_test.h/cc` | `task_manager_test.rs` |
| UrlRequestTest | `urlrequest_test.h/cc` | `urlrequest_test.rs` |
| WindowTest | `window_test.h/cc` | `window_test.rs` |

**Missing Rust implementations:**
- [ ] **ComponentTest** (`component_test.h/cc`) - UI component tests
- [ ] **SchemeTest** (`scheme_test.h/cc`) - custom scheme tests
- [ ] **PopupsTest** (part of `test_runner.cc`) - popup window tests

---

## Phase 5: Shared/Common Code

### 5.1 Common Module
**C++ files:** `common/scheme_test_common.h/cc`, `common/client_app_delegates_common.h/cc`

**Tasks:**
- [ ] Scheme test utilities (already partially in `common.rs`)
- [ ] Client app delegates for multi-process
- [ ] Resource loading utilities

### 5.2 Resource Utilities
**C++ files:** `resource.h`, `resource_util_linux.cc`, `resource_util_win_idmap.cc`

**Tasks:**
- [ ] Cross-platform resource loading
- [ ] Windows resource ID mapping
- [ ] Resource file integration

---

## Phase 6: OSR (Off-Screen Rendering) Deep Integration

### 6.1 OSR Renderer
**C++ files:** `osr_renderer.h/cc`, `osr_renderer_settings.h`

**Tasks:**
- [ ] Complete `OsrRenderer` implementation (partial in Rust)
- [ ] D3D11 compositing (Windows)
- [ ] Platform-specific render target creation
- [ ] Paint event handling and bitmap conversion
- [ ] Accessibility node support

### 6.2 OSR Accessibility
**C++ files:** `osr_accessibility_helper.h/cc`, `osr_accessibility_node.h/cc`, `osr_accessibility_node_mac.mm`, `osr_accessibility_node_win.cc`

**Tasks:**
- [ ] Accessibility tree construction
- [ ] Platform accessibility integration (UIA on Windows, AX on macOS)

---

## Phase 7: Resources

### 7.1 HTML/JS Test Resources
**C++ resources:** `resources/*.html`

**Tasks:**
- [ ] Copy all HTML test pages from C++ version
- [ ] Verify JavaScript test scripts
- [ ] Update resource embedding in `build.rs`

### 7.2 Platform Resources
- [ ] Windows: `.ico`, `.rc` resources (already partially in `build.rs`)
- [ ] macOS: `Info.plist`, `MainMenu.xib` (already exists)

---

## Priority Order for Implementation

| Priority | Phase | Reason |
|----------|-------|--------|
| **P0** | 1.1 MainContext | Foundation for everything |
| **P0** | 1.3 ClientHandler | Core browser functionality |
| **P1** | 3.1 Views Framework | Cross-platform UI, what modern cefclient uses |
| **P1** | 4.1 Test Runner | Testing infrastructure |
| **P2** | 2.1-2.3 Platform Windows | Native UI for each platform |
| **P2** | 4.2 Missing Tests | Complete feature parity |
| **P3** | 5. Shared Code | Cleanup and utilities |
| **P3** | 6 OSR Deep | Advanced rendering |
| **P3** | 7 Resources | Polish and completeness |

---

## Key Architectural Considerations

1. **Threading Model**: CEF has strict threading requirements. Many operations must occur on specific threads (UI, IO, File). The Rust port must track and enforce thread affinity.

2. **Multi-Process**: cefclient runs browser, renderer, and helper processes. The port must handle inter-process communication.

3. **Handler Chaining**: CEF handlers often chain to default implementations. The Rust `wrap_*!` macro pattern is good but needs completeness.

4. **Platform Abstraction**: Use conditional compilation (`#[cfg(target_os = "...")]`) for platform-specific code, similar to the C++ version.

5. **RAII and Lifetime**: Rust's ownership model is an advantage over C++'s `CefRefPtr`. Leverage it for automatic cleanup.

---

## Implementation Order Summary

1. **Phase 1.3 - ClientHandler**: Add all CEF handler delegates to the client
2. **Phase 1.1 - MainContext**: Extract and unify application state management
3. **Phase 1.2 - RootWindowManager**: Add window tracking and management
4. **Phase 3.1 - Views Framework**: Implement cross-platform UI overlays
5. **Phase 4.1 - Test Runner**: Build test infrastructure
6. **Phase 4.2 - Missing Tests**: Complete remaining test implementations
7. **Phase 2 - Platform Windows**: Add native UI for each platform
8. **Phase 5 - Shared Code**: Clean up utilities
9. **Phase 6 - OSR**: Deep render integration
10. **Phase 7 - Resources**: Final polish