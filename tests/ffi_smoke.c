#include "m1_ffi.h"

#include <stdio.h>
#include <string.h>

typedef char* (*json_api)(const char*);

static int expect_error_envelope(const char* label, json_api call, const char* input)
{
    char* result = call(input);
    int failed = result == NULL || strstr(result, "\"ok\":false") == NULL ||
        strstr(result, "\"error\"") == NULL;
    if (failed) fprintf(stderr, "%s did not return an error envelope\n", label);
    m1_free_string(result);
    return failed;
}

int main(void)
{
    int failures = 0;
    if (m1_init_json(NULL) != NULL) {
        fprintf(stderr, "init(NULL) must return NULL\n");
        ++failures;
    }
    if (m1_process_json(NULL) != NULL) {
        fprintf(stderr, "process(NULL) must return NULL\n");
        ++failures;
    }
    if (m1_correct_json(NULL) != NULL) {
        fprintf(stderr, "correct(NULL) must return NULL\n");
        ++failures;
    }
    m1_free_string(NULL);

    failures += expect_error_envelope("init bad model path", m1_init_json,
        "{\"detectionModel\":\"/m1-smoke-missing/det.onnx\","
        "\"recognitionModel\":\"/m1-smoke-missing/rec.onnx\"}");
    failures += expect_error_envelope("invalid JSON", m1_init_json, "{");
    failures += expect_error_envelope("invalid process JSON", m1_process_json, "{");
    failures += expect_error_envelope("invalid correction JSON", m1_correct_json, "{");

    if (failures != 0) fprintf(stderr, "FFI smoke failed: %d check(s)\n", failures);
    return failures == 0 ? 0 : 1;
}