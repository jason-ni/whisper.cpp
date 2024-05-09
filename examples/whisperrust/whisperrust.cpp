//
// Created by jason on 5/7/24.
//
#include <stdio.h>
#include "lib.rs.h"

int main() {
    WhisperRust::run_transcript(std::string("output.wav"));
    return 0;
}