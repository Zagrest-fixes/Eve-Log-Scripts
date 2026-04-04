pub fn to_raw_text(logs: &[String]) -> Vec<String> {
    let mut out = vec![];
    for log in logs {   
        let mut chars = log.chars();
        while let Some(c) = chars.next() && c != ']' {}
        
        let mut cleaned = String::new();
        let mut in_tag = false;
        for c in chars {
            match (c, in_tag) {
                ('>', true) => in_tag = false,
                ('<', false) => in_tag = true,
                ('>', false) | ('<', true) => println!("Assumption about tags is wrong"),
                (_, true) => (),
                (_, false) => cleaned.push(c),
            }
        }
        out.push(cleaned);
    }
    out
}

