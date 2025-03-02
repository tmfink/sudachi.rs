use std::fs::{self, File};
use std::io::Read;
use std::path::Path;
use std::str::FromStr;

use crate::dic::grammar::Grammar;
use crate::dic::header::Header;
use crate::dic::lexicon::Lexicon;
use crate::lattice::node::Node;
use crate::lattice::Lattice;
use crate::morpheme::Morpheme;
use crate::prelude::*;

/// Able to tokenize Japanese text
pub trait Tokenize {
    /// Break text into `Morpheme`s
    fn tokenize(&self, input: &str, mode: Mode, enable_debug: bool)
        -> SudachiResult<Vec<Morpheme>>;
}

/// Tokenizes Japanese text
pub struct Tokenizer<'a> {
    pub grammar: Grammar<'a>,
    pub lexicon: Lexicon<'a>,
}

/// Unit to split text
///
/// Some examples:
/// ```text
/// A：選挙/管理/委員/会
/// B：選挙/管理/委員会
/// C：選挙管理委員会
///
/// A：客室/乗務/員
/// B：客室/乗務員
/// C：客室乗務員
///
/// A：労働/者/協同/組合
/// B：労働者/協同/組合
/// C：労働者協同組合
///
/// A：機能/性/食品
/// B：機能性/食品
/// C：機能性食品
/// ```
///
/// See [Sudachi documentation](https://github.com/WorksApplications/Sudachi#the-modes-of-splitting)
/// for more details
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Short
    A,

    /// Middle (similar to "word")
    B,

    /// Named Entity
    C,
}

impl FromStr for Mode {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "A" | "a" => Ok(Mode::A),
            "B" | "b" => Ok(Mode::B),
            "C" | "c" => Ok(Mode::C),
            _ => Err("Mode must be one of \"A\", \"B\", or \"C\" (in lower or upper case)."),
        }
    }
}

impl<'a> Tokenizer<'a> {
    /// Create `Tokenizer` from the raw bytes of a Sudachi dictionary.
    pub fn from_dictionary_bytes(dictionary_bytes: &'a [u8]) -> SudachiResult<Tokenizer<'a>> {
        let (_rest, _header) = Header::new(&dictionary_bytes[..Header::STORAGE_SIZE])?;
        let mut offset = Header::STORAGE_SIZE;

        let grammar = Grammar::new(dictionary_bytes, offset)?;
        offset += grammar.storage_size;

        let lexicon = Lexicon::new(dictionary_bytes, offset)?;

        Ok(Tokenizer { grammar, lexicon })
    }
}

/// Return bytes of a `dictionary_path`
pub fn dictionary_bytes_from_path<P: AsRef<Path>>(dictionary_path: P) -> SudachiResult<Vec<u8>> {
    let dictionary_path = dictionary_path.as_ref();
    let dictionary_stat = fs::metadata(&dictionary_path)?;
    let mut dictionary_file = File::open(dictionary_path)?;
    let mut dictionary_bytes = Vec::with_capacity(dictionary_stat.len() as usize);
    dictionary_file.read_to_end(&mut dictionary_bytes)?;

    Ok(dictionary_bytes)
}

impl<'a> Tokenize for Tokenizer<'a> {
    fn tokenize(
        &self,
        input: &str,
        mode: Mode,
        enable_debug: bool,
    ) -> SudachiResult<Vec<Morpheme>> {
        let input_bytes = input.as_bytes();

        // build_lattice
        let mut lattice = Lattice::new(&self.grammar, input_bytes.len());

        for (i, b) in input_bytes.iter().enumerate() {
            // TODO: if (!input.canBow(i) || !lattice.hasPreviousNode(i)) { continue; }
            if (b & 0xC0) == 0x80 {
                continue;
            }

            for (word_id, end) in self.lexicon.lookup(&input_bytes, i)? {
                let (left_id, right_id, cost) = self.lexicon.get_word_param(word_id as usize)?;
                let node = Node::new(left_id, right_id, cost, word_id);
                lattice.insert(i, end, node)?;
            }
        }
        lattice.connect_eos_node()?;

        // lattice dump
        if enable_debug {
            println!("=== Lattice dump:");
            let mut i = 0;
            for r_nodes in lattice.end_lists.iter().rev() {
                for r_node in r_nodes {
                    print!("{}: {}: ", i, r_node);
                    for l_node in &lattice.end_lists[r_node.begin] {
                        let connect_cost = self
                            .grammar
                            .get_connect_cost(l_node.right_id, r_node.left_id)?;
                        let cost = l_node.total_cost + connect_cost as i32;
                        print!("{} ", cost);
                    }
                    println!();
                    i += 1;
                }
            }
            println!("===");
        };

        let node_list = lattice.get_best_path()?;

        let mut word_id_list = Vec::new();
        if mode == Mode::C {
            word_id_list = node_list
                .iter()
                .map(|node| node.word_id.map(|x| x as usize))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| SudachiError::MissingWordId)?;
        } else {
            for node in &node_list {
                let node_word_id =
                    node.word_id.ok_or_else(|| SudachiError::MissingWordId)? as usize;
                let word_ids = match mode {
                    Mode::A => self.lexicon.get_word_info(node_word_id)?.a_unit_split,
                    Mode::B => self.lexicon.get_word_info(node_word_id)?.b_unit_split,
                    _ => unreachable!(),
                };

                if word_ids.is_empty() | (word_ids.len() == 1) {
                    word_id_list.push(node_word_id);
                } else {
                    for word_id in word_ids {
                        word_id_list.push(word_id as usize);
                    }
                }
            }
        };

        word_id_list
            .iter()
            .map(|word_id| Morpheme::new(*word_id, &self.grammar, &self.lexicon))
            .collect::<SudachiResult<Vec<_>>>()
    }
}
