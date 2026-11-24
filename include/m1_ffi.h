#pragma once
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Borrowed NUL-terminated UTF-8 JSON input. Returns an owned envelope string:
 * {ok:true,data:...} or {ok:false,error:{code,message}}. NULL input yields the
 * NULL_INPUT envelope; only allocation failure returns NULL. Free each result
 * exactly once with m1_free_string. Rust must contain panics and synchronize
 * shared state for worker threads. Await successful init before process/correct.
 */
char* m1_init_json(const char* input_json);
char* m1_process_json(const char* input_json);
char* m1_correct_json(const char* input_json);
/* NULL is accepted as a no-op. Do not pass pointers from other allocators. */
void m1_free_string(char* value);

#ifdef __cplusplus
}
#endif