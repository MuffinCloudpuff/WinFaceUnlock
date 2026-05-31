pub fn cosine_similarity(left: &[f32], right: &[f32]) -> Option<f32> {
    if left.len() != right.len() || left.is_empty() {
        return None;
    }

    let mut dot = 0.0_f32;
    let mut left_norm = 0.0_f32;
    let mut right_norm = 0.0_f32;

    for (left_value, right_value) in left.iter().zip(right.iter()) {
        dot += left_value * right_value;
        left_norm += left_value * left_value;
        right_norm += right_value * right_value;
    }

    if left_norm == 0.0 || right_norm == 0.0 {
        return None;
    }

    Some(dot / (left_norm.sqrt() * right_norm.sqrt()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_similarity_matches_identical_vectors() {
        let similarity = cosine_similarity(&[1.0, 0.0, 1.0], &[1.0, 0.0, 1.0]);

        let similarity = similarity.unwrap_or_default();
        assert!((similarity - 1.0).abs() < f32::EPSILON * 2.0);
    }

    #[test]
    fn cosine_similarity_rejects_mismatched_lengths() {
        let similarity = cosine_similarity(&[1.0], &[1.0, 2.0]);

        assert_eq!(similarity, None);
    }
}
