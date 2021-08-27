use crate::prelude::{Address, TryFrom, Vec, U256};
use rlp::{Decodable, DecoderError, Rlp};

#[cfg(feature = "london")]
pub(crate) mod eip_1559;
pub(crate) mod eip_2930;
pub(crate) mod legacy;

use crate::types::Wei;
use eip_2930::AccessTuple;

/// Typed Transaction Envelope (see https://eips.ethereum.org/EIPS/eip-2718)
#[derive(Eq, PartialEq)]
pub enum EthTransactionKind {
    Legacy(legacy::LegacyEthSignedTransaction),
    Eip2930(eip_2930::SignedTransaction2930),
    #[cfg(feature = "london")]
    Eip1559(eip_1559::SignedTransaction1559),
}

impl TryFrom<&[u8]> for EthTransactionKind {
    type Error = ParseTransactionError;

    #[cfg(feature = "london")]
    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        if bytes[0] == eip_2930::TYPE_BYTE {
            let tx_eip_2930 = eip_2930::SignedTransaction2930::decode(&Rlp::new(&bytes[1..]))?;
            Ok(Self::Eip2930(tx_eip_2930))
        } else if bytes[0] == eip_1559::TYPE_BYTE {
            let tx_eip_1559 = eip_1559::SignedTransaction1559::decode(&Rlp::new(&bytes[1..]))?;
            Ok(Self::Eip1559(tx_eip_1559))
        } else if bytes[0] <= 0x7f {
            Err(ParseTransactionError::UnknownTransactionType)
        } else if bytes[0] == 0xff {
            Err(ParseTransactionError::ReservedSentinel)
        } else {
            let tx_legacy = legacy::LegacyEthSignedTransaction::decode(&Rlp::new(bytes))?;
            Ok(Self::Legacy(tx_legacy))
        }
    }

    #[cfg(not(feature = "london"))]
    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        if bytes[0] == eip_2930::TYPE_BYTE {
            let access_list_tx = eip_2930::SignedTransaction2930::decode(&Rlp::new(&bytes[1..]))?;
            Ok(Self::Eip2930(access_list_tx))
        } else if bytes[0] <= 0x7f {
            Err(ParseTransactionError::UnknownTransactionType)
        } else if bytes[0] == 0xff {
            Err(ParseTransactionError::ReservedSentinel)
        } else {
            let legacy = legacy::LegacyEthSignedTransaction::decode(&Rlp::new(bytes))?;
            Ok(Self::Legacy(legacy))
        }
    }
}

#[cfg(not(feature = "london"))]
pub struct NormalizedEthTransaction {
    pub address: Option<Address>,
    pub chain_id: Option<u64>,
    pub nonce: U256,
    pub gas_price: U256,
    pub gas_limit: U256,
    pub to: Option<Address>,
    pub value: Wei,
    pub data: Vec<u8>,
    pub access_list: Vec<AccessTuple>,
}

#[cfg(not(feature = "london"))]
impl From<EthTransactionKind> for NormalizedEthTransaction {
    fn from(kind: EthTransactionKind) -> Self {
        use EthTransactionKind::*;
        match kind {
            Legacy(tx) => Self {
                address: tx.sender(),
                chain_id: None,
                nonce: tx.transaction.nonce,
                gas_price: tx.transaction.gas_price,
                gas_limit: tx.transaction.gas_limit,
                to: tx.transaction.to,
                value: tx.transaction.value,
                data: tx.transaction.data,
                access_list: vec![],
            },
            Eip2930(tx) => Self {
                address: tx.sender(),
                chain_id: Some(tx.transaction.chain_id),
                nonce: tx.transaction.nonce,
                gas_price: tx.transaction.gas_price,
                gas_limit: tx.transaction.gas_limit,
                to: tx.transaction.to,
                value: tx.transaction.value,
                data: tx.transaction.data,
                access_list: tx.transaction.access_list,
            },
        }
    }
}

/// A normalized Ethereum transaction which can be created from older
/// transactions.
#[cfg(feature = "london")]
pub struct NormalizedEthTransaction {
    pub address: Option<Address>,
    pub chain_id: Option<u64>,
    pub nonce: U256,
    pub gas_limit: U256,
    pub max_priority_fee_per_gas: U256,
    pub max_fee_per_gas: U256,
    pub to: Option<Address>,
    pub value: Wei,
    pub data: Vec<u8>,
    pub access_list: Vec<AccessTuple>,
}

#[cfg(feature = "london")]
impl From<EthTransactionKind> for NormalizedEthTransaction {
    fn from(kind: EthTransactionKind) -> Self {
        use EthTransactionKind::*;
        match kind {
            Legacy(tx) => Self {
                address: tx.sender(),
                chain_id: None, // This should be fine as it'll fail regardless.
                nonce: tx.transaction.nonce,
                gas_limit: tx.transaction.gas_limit,
                max_priority_fee_per_gas: tx.transaction.gas_price,
                max_fee_per_gas: tx.transaction.gas_price,
                to: tx.transaction.to,
                value: tx.transaction.value,
                data: tx.transaction.data,
                access_list: vec![],
            },
            Eip2930(tx) => Self {
                address: tx.sender(),
                chain_id: Some(tx.transaction.chain_id),
                nonce: tx.transaction.nonce,
                gas_limit: tx.transaction.gas_limit,
                max_priority_fee_per_gas: tx.transaction.gas_price,
                max_fee_per_gas: tx.transaction.gas_price,
                to: tx.transaction.to,
                value: tx.transaction.value,
                data: tx.transaction.data,
                access_list: tx.transaction.access_list,
            },
            Eip1559(tx) => Self {
                address: tx.sender(),
                chain_id: Some(tx.transaction.chain_id),
                nonce: tx.transaction.nonce,
                gas_limit: tx.transaction.gas_limit,
                max_priority_fee_per_gas: tx.transaction.max_priority_fee_per_gas,
                max_fee_per_gas: tx.transaction.max_fee_per_gas,
                to: tx.transaction.to,
                value: tx.transaction.value,
                data: tx.transaction.data,
                access_list: tx.transaction.access_list,
            },
        }
    }
}

impl NormalizedEthTransaction {
    pub fn intrinsic_gas(&self, config: &evm::Config) -> Option<u64> {
        let is_contract_creation = self.to.is_none();

        let base_gas = if is_contract_creation {
            config.gas_transaction_create
        } else {
            config.gas_transaction_call
        };

        let num_zero_bytes = self.data.iter().filter(|b| **b == 0).count();
        let num_non_zero_bytes = self.data.len() - num_zero_bytes;

        let gas_zero_bytes = config
            .gas_transaction_zero_data
            .checked_mul(num_zero_bytes as u64)?;
        let gas_non_zero_bytes = config
            .gas_transaction_non_zero_data
            .checked_mul(num_non_zero_bytes as u64)?;

        let gas_access_list_address = config
            .gas_access_list_address
            .checked_mul(self.access_list.len() as u64)?;
        let gas_access_list_storage = config.gas_access_list_storage_key.checked_mul(
            self.access_list
                .iter()
                .map(|a| a.storage_keys.len() as u64)
                .sum(),
        )?;

        base_gas
            .checked_add(gas_zero_bytes)
            .and_then(|gas| gas.checked_add(gas_non_zero_bytes))
            .and_then(|gas| gas.checked_add(gas_access_list_address))
            .and_then(|gas| gas.checked_add(gas_access_list_storage))
    }
}

pub enum ParseTransactionError {
    UnknownTransactionType,
    // Per the EIP-2718 spec 0xff is a reserved value
    ReservedSentinel,
    RlpDecodeError(DecoderError),
}

impl From<DecoderError> for ParseTransactionError {
    fn from(e: DecoderError) -> Self {
        Self::RlpDecodeError(e)
    }
}

impl AsRef<[u8]> for ParseTransactionError {
    fn as_ref(&self) -> &[u8] {
        match self {
            Self::UnknownTransactionType => b"ERR_UNKNOWN_TX_TYPE",
            Self::ReservedSentinel => b"ERR_RESERVED_LEADING_TX_BYTE",
            Self::RlpDecodeError(_) => b"ERR_TX_RLP_DECODE",
        }
    }
}

fn rlp_extract_to(rlp: &Rlp<'_>, index: usize) -> Result<Option<Address>, DecoderError> {
    let value = rlp.at(index)?;
    if value.is_empty() {
        if value.is_data() {
            Ok(None)
        } else {
            Err(rlp::DecoderError::RlpExpectedToBeData)
        }
    } else {
        let v: Address = value.as_val()?;
        if v == Address::zero() {
            Ok(None)
        } else {
            Ok(Some(v))
        }
    }
}

fn vrs_to_arr(v: u8, r: U256, s: U256) -> [u8; 65] {
    let mut result = [0u8; 65]; // (r, s, v), typed (uint256, uint256, uint8)
    r.to_big_endian(&mut result[0..32]);
    s.to_big_endian(&mut result[32..64]);
    result[64] = v;
    result
}
