#include "napi/native_api.h"
#include "m1_ffi.h"

#include <algorithm>
#include <memory>
#include <new>
#include <vector>

namespace {
constexpr size_t MAX_REQUEST_BYTES = 32U * 1024U * 1024U;
using RustCall = char* (*)(const char*);

// Never log inputs, model paths, images, corrections or Rust outputs.
struct Request {
    napi_async_work work = nullptr;
    napi_deferred deferred = nullptr;
    RustCall call = nullptr;
    std::vector<char> input;
    char* output = nullptr;
    bool nativeFailure = false;
    ~Request() { if (output != nullptr) m1_free_string(output); }
};

napi_value ThrowNative(napi_env env, const char* message)
{
    bool pending = false;
    if (napi_is_exception_pending(env, &pending) == napi_ok && pending) return nullptr;
    napi_throw_error(env, "ERR_M1_NATIVE", message);
    return nullptr;
}

void RejectNative(napi_env env, napi_deferred deferred, const char* message)
{
    napi_value error = nullptr;
    bool pending = false;
    if (napi_is_exception_pending(env, &pending) == napi_ok && pending) {
        if (napi_get_and_clear_last_exception(env, &error) != napi_ok) return;
    } else {
        napi_value text = nullptr;
        napi_value code = nullptr;
        if (napi_create_string_utf8(env, message, NAPI_AUTO_LENGTH, &text) != napi_ok ||
            napi_create_string_utf8(env, "ERR_M1_NATIVE", NAPI_AUTO_LENGTH, &code) != napi_ok ||
            napi_create_error(env, code, text, &error) != napi_ok) {
            ThrowNative(env, "failed to create native error");
            return;
        }
    }
    if (napi_reject_deferred(env, deferred, error) != napi_ok) {
        ThrowNative(env, "failed to reject native promise");
    }
}

void Execute(napi_env, void* data)
{
    auto* request = static_cast<Request*>(data);
    // Worker thread: no JS/N-API calls. Rust must contain its own panics.
    try {
        request->output = request->call(request->input.data());
        request->nativeFailure = request->output == nullptr;
    } catch (...) {
        request->nativeFailure = true;
    }
}

void Complete(napi_env env, napi_status status, void* data)
{
    std::unique_ptr<Request> request(static_cast<Request*>(data));
    if (status != napi_ok || request->nativeFailure) {
        RejectNative(env, request->deferred, "native work failed or was cancelled");
    } else {
        napi_value result = nullptr;
        if (napi_create_string_utf8(env, request->output, NAPI_AUTO_LENGTH, &result) != napi_ok) {
            RejectNative(env, request->deferred, "failed to create native result");
        } else if (napi_resolve_deferred(env, request->deferred, result) != napi_ok) {
            RejectNative(env, request->deferred, "failed to resolve native result");
        }
        // Business errors are envelopes too; pass through without parsing.
    }
    if (napi_delete_async_work(env, request->work) != napi_ok) {
        ThrowNative(env, "failed to release native work");
    }
    // Destructor frees the Rust-owned output on every completion path.
}

napi_value Schedule(napi_env env, napi_callback_info info, RustCall call, const char* name)
{
    size_t argc = 2;
    napi_value argv[2] = {nullptr, nullptr};
    if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok) {
        return ThrowNative(env, "failed to read native arguments");
    }
    if (argc != 1) {
        napi_throw_type_error(env, "ERR_M1_ARGUMENT", "expected exactly one JSON string");
        return nullptr;
    }
    napi_valuetype type = napi_undefined;
    if (napi_typeof(env, argv[0], &type) != napi_ok) {
        return ThrowNative(env, "failed to inspect native argument");
    }
    if (type != napi_string) {
        napi_throw_type_error(env, "ERR_M1_ARGUMENT", "request must be a JSON string");
        return nullptr;
    }
    size_t length = 0;
    if (napi_get_value_string_utf8(env, argv[0], nullptr, 0, &length) != napi_ok) {
        return ThrowNative(env, "failed to measure request");
    }
    if (length > MAX_REQUEST_BYTES) {
        napi_throw_range_error(env, "ERR_M1_REQUEST_SIZE", "request exceeds 32 MiB of UTF-8");
        return nullptr;
    }

    try {
        auto request = std::make_unique<Request>();
        request->call = call;
        request->input.resize(length + 1, '\0');
        size_t copied = 0;
        if (napi_get_value_string_utf8(env, argv[0], request->input.data(),
                                       request->input.size(), &copied) != napi_ok || copied != length) {
            return ThrowNative(env, "failed to copy request");
        }
        // The C ABI uses NUL termination; do not silently truncate JSON.
        if (std::find(request->input.begin(), request->input.end() - 1, '\0') !=
            request->input.end() - 1) {
            napi_throw_type_error(env, "ERR_M1_ARGUMENT", "request contains an embedded NUL");
            return nullptr;
        }
        napi_value resourceName = nullptr;
        if (napi_create_string_utf8(env, name, NAPI_AUTO_LENGTH, &resourceName) != napi_ok) {
            return ThrowNative(env, "failed to create work name");
        }
        napi_value promise = nullptr;
        if (napi_create_promise(env, &request->deferred, &promise) != napi_ok) {
            return ThrowNative(env, "failed to create native promise");
        }
        if (napi_create_async_work(env, nullptr, resourceName, Execute, Complete,
                                   request.get(), &request->work) != napi_ok) {
            RejectNative(env, request->deferred, "failed to create native work");
            return promise;
        }
        if (napi_queue_async_work(env, request->work) != napi_ok) {
            RejectNative(env, request->deferred, "failed to queue native work");
            if (napi_delete_async_work(env, request->work) != napi_ok) {
                ThrowNative(env, "failed to release unqueued native work");
            }
            return promise;
        }
        request.release(); // Complete now owns the request and async work.
        return promise;
    } catch (const std::bad_alloc&) {
        return ThrowNative(env, "native request allocation failed");
    } catch (...) {
        return ThrowNative(env, "native request setup failed");
    }
}

napi_value Initialize(napi_env env, napi_callback_info info)
{
    return Schedule(env, info, m1_init_json, "m1.init");
}
napi_value ProcessPhoto(napi_env env, napi_callback_info info)
{
    return Schedule(env, info, m1_process_json, "m1.processPhoto");
}
napi_value CorrectEvidence(napi_env env, napi_callback_info info)
{
    return Schedule(env, info, m1_correct_json, "m1.correctEvidence");
}

napi_value RegisterExports(napi_env env, napi_value exports)
{
    const napi_property_descriptor properties[] = {
        {"init", nullptr, Initialize, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"processPhoto", nullptr, ProcessPhoto, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"correctEvidence", nullptr, CorrectEvidence, nullptr, nullptr, nullptr, napi_default, nullptr},
    };
    if (napi_define_properties(env, exports, sizeof(properties) / sizeof(properties[0]), properties) != napi_ok) {
        return ThrowNative(env, "failed to register native exports");
    }
    return exports;
}

// Positional aggregate initialization is valid C++17.
napi_module module = {1, 0, nullptr, RegisterExports, "xiaohong_perception", nullptr, {0}};
} // namespace

extern "C" __attribute__((constructor)) void RegisterXiaohongPerceptionModule()
{
    napi_module_register(&module);
}