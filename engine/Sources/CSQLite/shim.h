// The system's SQLite: libsqlite3 on macOS and Linux, winsqlite3 (part of Windows 10 and later) on Windows.
#if defined(_WIN32)
#include <winsqlite/winsqlite3.h>
#else
#include <sqlite3.h>
#endif
