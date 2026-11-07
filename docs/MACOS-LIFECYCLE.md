# Mac accessibility adapter and native lifecycle

## Observed failure

Enabling eframe 0.36.2's AccessKit feature on macOS 26.6.2 reproduced an AppKit
NSRangeException during a native viewport-close run. AppKit attempted to remove
a nextResponder observer from WinitView that it did not recognize as registered.
The user's supplied crash report matches the process, application version,
timestamp and exception stack from that test. Model fitting was not on the
exception stack. This does not establish immunity from unrelated numerical bugs.

The native dependency includes accesskit_macos 0.26.3's SubclassingAdapter,
which changes and restores the native view's class. Reports of the same exception
exist in other applications using that integration:
https://github.com/storytold/craft-libs/issues/3
https://github.com/storytold/lightcraft/issues/1331
These are supporting reports, not a substitute for this application's tests.

## Change and limit

Cargo enables the AccessKit feature on non-Mac targets only. The Mac package
retains its previous stable native view behavior. Windows retains the native
accessibility adapter. This avoids changing system settings or intercepting OS
exceptions. It also means full native VoiceOver support is not established for
this Mac release; do not describe it as accessible merely because keyboard
controls remain available.

Restore the Mac adapter only after an upstream repair or a separately reviewed
native integration passes both assistive-technology and lifecycle tests. Do not
hide the exception by unconditionally forcing process exit in the product.

## Regression check

verify_macos_lifecycle.py launches the real packaged executable ten times,
alternating normal and compact windows. Each launch fits all nine models,
renders six views, captures an image and requests native viewport Close.
Every subprocess must exit zero and produce a complete report. The bounded
result is saved in verification-macos-lifecycle.json. This tests the recorded
machine and close path; it does not establish compatibility with every macOS
release or assistive configuration. Any later change to native accessibility,
window construction or dependency versions must rerun this check.
