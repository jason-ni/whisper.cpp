//
// Created by jason on 5/7/24.
//
#pragma once

#include <memory>
#include "rust/cxx.h"
#include "whisper.h"
//#include "common.h"


namespace WhisperRust {

    struct SenderWrapper;

    class WhisperWrapper {
    public:
        explicit WhisperWrapper(const std::string& model_path);
        ~WhisperWrapper();

        int32_t infer_buffer(const SenderWrapper &sender, const float* buffer, size_t buffer_size) const;
        int32_t get_segment_count() const;
        int progress_ = 0;
    private:
        std::string prompt_;
        struct whisper_context* whisper_ctx_;
    };

    std::unique_ptr<WhisperWrapper> create_whisper_wrapper(rust::Str model_path);
}