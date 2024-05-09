#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::{
    __m128i, _mm_loadu_si128,
    __m256, __m256i, _mm256_cvtepi16_epi32,
    _mm256_cvtepi32_ps, _mm256_mul_ps, _mm256_set1_ps,
};

#[cfg(target_arch = "x86_64")]
fn simd_convert_pcm16_to_f32(data: &[i16], target: &mut [f32]) {
    let scale = 32768.0f32;
    let step_cnt =  data.len() / 8;
    for i in (0..step_cnt) {
        unsafe {
            let x: __m128i = _mm_loadu_si128(data[i*8..i*8+8].as_ptr() as *const __m128i);
            let y: __m256i = _mm256_cvtepi16_epi32(x); // convert to i32
            let z: __m256 = _mm256_cvtepi32_ps(y); // convert to f32
            let scale: __m256 = _mm256_set1_ps(1.0f32/ scale);
            let w: __m256 = _mm256_mul_ps(z, scale); // divide by scale
            let w_arr: [f32; 8] = std::mem::transmute(w);
            target[i*8..i*8+8].copy_from_slice(&w_arr[0..8]);
        }
    }
    for i in (step_cnt*8..data.len()) {
        target[i] = data[i] as f32 / scale;
    }
}

pub fn convert_pcm16_to_f32(data: &[i16], target: &mut [f32]) {
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") {
            return simd_convert_pcm16_to_f32(data, target);
        }
    }
    for i in 0..data.len() {
        target[i] = data[i] as f32 / 32768.0;
    }
}
