//
// Created by jason on 5/7/24.
//

#include <thread>
#include "whisper_wrapper.h"

namespace WhisperRust {

    WhisperWrapper::WhisperWrapper(const std::string& model_path) {
        struct whisper_context_params cparams = whisper_context_default_params();
        cparams.use_gpu = true;

        whisper_ctx_ = whisper_init_from_file_with_params(model_path.c_str(), cparams);

    }

    WhisperWrapper::~WhisperWrapper() {
        if (whisper_ctx_) {
            whisper_free(whisper_ctx_);
        }
    }

    struct print_user_data {
        int progress;
    };

    void whisper_print_progress_callback(struct whisper_context * /*ctx*/, struct whisper_state * /*state*/, int progress, void * user_data) {
        int progress_prev = ((print_user_data*) user_data)->progress;
        if (progress >= progress_prev + 10) {
            ((print_user_data*) user_data)->progress += 10;
            fprintf(stderr, "%s: progress = %3d%%\n", __func__, progress);
        }
    }

    void whisper_print_segment_callback(struct whisper_context * ctx, struct whisper_state * /*state*/, int n_new, void * user_data) {
        const int n_segments = whisper_full_n_segments(ctx);
        printf("new segments: %d\n", n_segments);

        int64_t t0 = 0;
        int64_t t1 = 0;

        // print the last n_new segments
        const int s0 = n_segments - n_new;

        if (s0 == 0) {
            printf("\n");
        }

        for (int i = s0; i < n_segments; i++) {
            printf("[%s --> %s]  ", to_timestamp(t0).c_str(), to_timestamp(t1).c_str());

            const char * text = whisper_full_get_segment_text(ctx, i);
            printf("%s", text);

            fflush(stdout);
        }
    }

    int32_t WhisperWrapper::infer_buffer(const float *buffer, size_t buffer_size) const {
        whisper_full_params wparams = whisper_full_default_params(WHISPER_SAMPLING_GREEDY);

        wparams.strategy = WHISPER_SAMPLING_BEAM_SEARCH;

        wparams.print_realtime   = false;
        wparams.print_progress   = true;
        wparams.print_timestamps = true;
        wparams.print_special    = false;
        wparams.translate        = false;
        wparams.language         = "auto";
        wparams.detect_language  = false;
        wparams.n_threads        = 4;
        wparams.offset_ms        = 0;
        wparams.duration_ms      = 0;
        wparams.debug_mode       = true;

        wparams.token_timestamps = false;
        wparams.thold_pt         = 0.01f;
        wparams.max_len          = 120;
        wparams.split_on_word    = false;
        wparams.audio_ctx        = 0;



        wparams.initial_prompt   = prompt_.c_str();

        wparams.greedy.best_of        = 5;
        wparams.beam_search.beam_size = 5;

        //wparams.temperature_inc  = params.no_fallback ? 0.0f : wparams.temperature_inc;
        wparams.entropy_thold    = 2.40f;
        wparams.logprob_thold    = -1.00f;

        wparams.no_timestamps    = true;

        print_user_data user_data = {0};

        // this callback is called on each new segment
        if (!wparams.print_realtime) {
            wparams.new_segment_callback           = whisper_print_segment_callback;
            wparams.new_segment_callback_user_data = &user_data;
        }


        if (wparams.print_progress) {
            wparams.progress_callback           = whisper_print_progress_callback;
            wparams.progress_callback_user_data = &user_data;
        }

        // examples for abort mechanism
        // in examples below, we do not abort the processing, but we could if the flag is set to true

        // the callback is called before every encoder run - if it returns false, the processing is aborted
        {
            static bool is_aborted = false; // NOTE: this should be atomic to avoid data race

            wparams.encoder_begin_callback = [](struct whisper_context * /*ctx*/, struct whisper_state * /*state*/, void * user_data) {
                printf("encoder begin \n");
                bool is_aborted = *(bool*)user_data;
                return !is_aborted;
            };
            wparams.encoder_begin_callback_user_data = &is_aborted;
        }

        // the callback is called before every computation - if it returns true, the computation is aborted
        {
            static bool is_aborted = false; // NOTE: this should be atomic to avoid data race

            wparams.abort_callback = [](void * user_data) {
                bool is_aborted = *(bool*)user_data;
                return is_aborted;
            };
            wparams.abort_callback_user_data = &is_aborted;
        }

        printf("infer buffer size %ld\n", buffer_size);
        return whisper_full(whisper_ctx_, wparams, buffer, buffer_size);
    }

    int32_t WhisperWrapper::get_segment_count() const {
        return whisper_full_n_segments(whisper_ctx_);
    }

    std::unique_ptr<WhisperWrapper> create_whisper_wrapper(rust::Str model_path) {
        return std::unique_ptr<WhisperWrapper>(new WhisperWrapper(std::string(model_path)));
    }
}
