use std::sync::OnceLock;
pub struct Question {
    pub id: usize,
    pub topic: String,
    pub title: String,
    pub stem: String,
    pub options: [String; 4],
    pub correct: Vec<usize>,
    pub explanation: String,
    pub code: String,
}
pub fn questions() -> &'static [Question] {
    static BANK: OnceLock<Vec<Question>> = OnceLock::new();
    BANK.get_or_init(|| {
        let bank: serde_json::Value = serde_json::from_str(include_str!("../questions.json"))
            .expect("Authored question bank must be valid JSON");
        bank.as_array()
            .unwrap()
            .iter()
            .map(|q| Question {
                id: q["id"].as_u64().unwrap() as usize,
                topic: q["topic"].as_str().unwrap().into(),
                title: q["title"].as_str().unwrap().into(),
                stem: q["stem"].as_str().unwrap().into(),
                options: std::array::from_fn(|i| q["options"][i].as_str().unwrap().into()),
                correct: q["correct"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_u64().unwrap() as usize)
                    .collect(),
                explanation: q["explanation"].as_str().unwrap().into(),
                code: q["code"].as_str().unwrap_or("").into(),
            })
            .collect()
    })
}
#[cfg(test)]
mod tests {
    #[test]
    fn authored_hundred_question_bank_is_complete_and_distinct() {
        let bank = super::questions();
        assert_eq!(bank.len(), 100);
        let mut titles = std::collections::HashSet::new();
        let mut topics = std::collections::HashMap::new();
        for (i, q) in bank.iter().enumerate() {
            assert_eq!(q.id, i + 1);
            assert!(titles.insert(&q.title));
            *topics.entry(&q.topic).or_insert(0) += 1;
            assert!(!q.correct.is_empty());
            assert!(q.correct.iter().all(|&i| i < 4));
            assert!(!q.explanation.is_empty());
            assert!(q.options.iter().all(|s| !s.is_empty()));
        }
        assert_eq!(topics.len(), 10);
        assert!(topics.values().all(|&n| n == 10));
        assert!(bank.iter().filter(|q| !q.code.is_empty()).count() >= 12);
    }
}
