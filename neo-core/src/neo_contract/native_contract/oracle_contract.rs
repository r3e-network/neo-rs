use neo_contract::prelude::*;
use neo_contract::storage::{StorageKey, StorageItem};
use neo_contract::vm::types::{Array, StackItem};
use neo_contract::crypto::Crypto;
use neo_contract::binary_serializer::BinarySerializer;
use neo_contract::contract_management::ContractManagement;
use neo_contract::gas::GAS;
use neo_contract::role_management::RoleManagement;
use std::collections::HashSet;

#[contract]
pub struct OracleContract {
    max_url_length: u32,
    max_filter_length: u32,
    max_callback_length: u32,
    max_user_data_length: u32,
    prefix_price: u8,
    prefix_request_id: u8,
    prefix_request: u8,
    prefix_id_list: u8,
}

#[contract_event(0, name = "OracleRequest")]
pub struct OracleRequestEvent {
    id: u64,
    request_contract: H160,
    url: String,
    filter: Option<String>,
}

#[contract_event(1, name = "OracleResponse")]
pub struct OracleResponseEvent {
    id: u64,
    original_tx: H256,
}

#[contract]
impl OracleContract {
    pub fn new() -> Self {
        Self {
            max_url_length: 256,
            max_filter_length: 128,
            max_callback_length: 32,
            max_user_data_length: 512,
            prefix_price: 5,
            prefix_request_id: 9,
            prefix_request: 7,
            prefix_id_list: 6,
        }
    }

    #[contract_method(cpu_fee = 1 << 15, required_flags = CallFlags::STATES)]
    fn set_price(&mut self, engine: &mut ApplicationEngine, price: i64) -> Result<(), String> {
        if price <= 0 {
            return Err("Price must be positive".into());
        }
        if !self.check_committee(engine) {
            return Err("Not authorized".into());
        }
        engine.snapshot_cache().get_and_change(self.create_storage_key(self.prefix_price)).set(price);
        Ok(())
    }

    #[contract_method(cpu_fee = 1 << 15, required_flags = CallFlags::READ_STATES)]
    pub fn get_price(&self, snapshot: &dyn DataCache) -> i64 {
        snapshot.get(&self.create_storage_key(self.prefix_price))
            .map(|item| item.as_integer().unwrap())
            .unwrap_or(0)
    }

    #[contract_method(required_flags = CallFlags::STATES | CallFlags::ALLOW_CALL | CallFlags::ALLOW_NOTIFY)]
    fn finish(&mut self, engine: &mut ApplicationEngine) -> Result<ContractTask, String> {
        if engine.invocation_stack().len() != 2 {
            return Err("Invalid invocation stack".into());
        }
        if engine.get_invocation_counter() != 1 {
            return Err("Invalid invocation counter".into());
        }
        let tx = engine.script_container().as_transaction().ok_or("Not a transaction")?;
        let response = tx.get_attribute::<OracleResponse>().ok_or("Oracle response not found")?;
        let request = self.get_request(engine.snapshot_cache(), response.id).ok_or("Oracle request not found")?;
        
        engine.send_notification(self.hash(), "OracleResponse", Array::new_with_items(vec![
            StackItem::Integer(response.id.into()),
            StackItem::ByteArray(request.original_txid.to_vec()),
        ]));

        let user_data = BinarySerializer::deserialize(&request.user_data, engine.limits(), engine.reference_counter())?;
        
        engine.call_from_native_contract_async(
            self.hash(),
            &request.callback_contract,
            &request.callback_method,
            vec![
                StackItem::String(request.url),
                user_data,
                StackItem::Integer(response.code as i32),
                StackItem::ByteArray(response.result),
            ],
        )
    }

    fn get_original_txid(&self, engine: &ApplicationEngine) -> H256 {
        let tx = engine.script_container().as_transaction().unwrap();
        match tx.get_attribute::<OracleResponse>() {
            Some(response) => {
                let request = self.get_request(engine.snapshot_cache(), response.id).unwrap();
                request.original_txid
            },
            None => tx.hash(),
        }
    }

    pub fn get_request(&self, snapshot: &dyn DataCache, id: u64) -> Option<OracleRequest> {
        snapshot.try_get(&self.create_storage_key(self.prefix_request).add_big_endian(id))
            .map(|item| item.get_interoperable())
    }

    pub fn get_requests(&self, snapshot: &dyn DataCache) -> Vec<(u64, OracleRequest)> {
        snapshot.find(self.create_storage_key(self.prefix_request).to_vec())
            .map(|(key, value)| {
                let id = u64::from_be_bytes(key[1..9].try_into().unwrap());
                let request: OracleRequest = value.get_interoperable();
                (id, request)
            })
            .collect()
    }

    pub fn get_requests_by_url(&self, snapshot: &dyn DataCache, url: &str) -> Vec<(u64, OracleRequest)> {
        let list: Option<IdList> = snapshot.try_get(&self.create_storage_key(self.prefix_id_list).add(&self.get_url_hash(url)))
            .map(|item| item.get_interoperable());
        
        match list {
            Some(id_list) => id_list.iter()
                .filter_map(|&id| {
                    self.get_request(snapshot, id)
                        .map(|request| (id, request))
                })
                .collect(),
            None => Vec::new(),
        }
    }

    fn get_url_hash(url: &str) -> Vec<u8> {
        Crypto::hash160(url.as_bytes())
    }

    #[contract_method(cpu_fee = 1 << 15)]
    fn initialize(&mut self, engine: &mut ApplicationEngine, hardfork: Option<Hardfork>) -> Result<(), String> {
        if hardfork == Some(self.active_in()) {
            engine.snapshot_cache().add(self.create_storage_key(self.prefix_request_id), StorageItem::new(0u64));
            engine.snapshot_cache().add(self.create_storage_key(self.prefix_price), StorageItem::new(50_000_000i64));
        }
        Ok(())
    }

    #[contract_method]
    async fn post_persist(&mut self, engine: &mut ApplicationEngine) -> Result<(), String> {
        let mut nodes: Option<Vec<(H160, u64)>> = None;

        for tx in engine.persisting_block().transactions() {
            let response = match tx.get_attribute::<OracleResponse>() {
                Some(r) => r,
                None => continue,
            };

            let key = self.create_storage_key(self.prefix_request).add_big_endian(response.id);
            let request = match engine.snapshot_cache().try_get(&key) {
                Some(item) => item.get_interoperable::<OracleRequest>(),
                None => continue,
            };
            engine.snapshot_cache().delete(&key);

            let id_list_key = self.create_storage_key(self.prefix_id_list).add(&Self::get_url_hash(&request.url));
            let mut id_list = engine.snapshot_cache().get_and_change(&id_list_key).get_interoperable::<IdList>();
            if !id_list.remove(&response.id) {
                return Err("Invalid state".into());
            }
            if id_list.is_empty() {
                engine.snapshot_cache().delete(&id_list_key);
            }

            if nodes.is_none() {
                nodes = Some(RoleManagement::get_designated_by_role(engine.snapshot_cache(), Role::Oracle, engine.persisting_block().index())
                    .into_iter()
                    .map(|pubkey| (Contract::create_signature_redeem_script(&pubkey).to_script_hash(), 0u64))
                    .collect());
            }

            if let Some(ref mut node_list) = nodes {
                if !node_list.is_empty() {
                    let index = (response.id % node_list.len() as u64) as usize;
                    node_list[index].1 += self.get_price(engine.snapshot_cache()) as u64;
                }
            }
        }

        if let Some(node_list) = nodes {
            for (account, gas) in node_list {
                if gas > 0 {
                    GAS::mint(engine, &account, gas, false).await?;
                }
            }
        }

        Ok(())
    }

    #[contract_method(required_flags = CallFlags::STATES | CallFlags::ALLOW_NOTIFY)]
    async fn request(&mut self, engine: &mut ApplicationEngine, url: String, filter: Option<String>, callback: String, user_data: StackItem, gas_for_response: i64) -> Result<(), String> {
        if url.len() > self.max_url_length as usize
            || filter.as_ref().map_or(false, |f| f.len() > self.max_filter_length as usize)
            || callback.len() > self.max_callback_length as usize
            || callback.starts_with('_')
            || gas_for_response < 10_000_000
        {
            return Err("Invalid arguments".into());
        }

        engine.add_fee(self.get_price(engine.snapshot_cache()));

        engine.add_fee(gas_for_response);
        GAS::mint(engine, &self.hash(), gas_for_response as u64, false).await?;

        let mut item_id = engine.snapshot_cache().get_and_change(&self.create_storage_key(self.prefix_request_id));
        let id = item_id.as_integer().unwrap();
        item_id.add(1);

        if ContractManagement::get_contract(engine.snapshot_cache(), &engine.calling_script_hash()).is_none() {
            return Err("Invalid calling script".into());
        }

        let request = OracleRequest {
            original_txid: self.get_original_txid(engine),
            gas_for_response: gas_for_response as u64,
            url: url.clone(),
            filter: filter.clone(),
            callback_contract: engine.calling_script_hash(),
            callback_method: callback,
            user_data: BinarySerializer::serialize(&user_data, self.max_user_data_length, engine.limits().max_stack_size())?,
        };

        engine.snapshot_cache().add(
            self.create_storage_key(self.prefix_request).add_big_endian(id),
            StorageItem::new(request),
        );

        let mut id_list = engine.snapshot_cache()
            .get_and_change(&self.create_storage_key(self.prefix_id_list).add(&Self::get_url_hash(&url)), || StorageItem::new(IdList::new()))
            .get_interoperable::<IdList>();

        if id_list.len() >= 256 {
            return Err("Too many pending responses for this URL".into());
        }
        id_list.insert(id);

        engine.send_notification(self.hash(), "OracleRequest", Array::new_with_items(vec![
            StackItem::Integer(id),
            StackItem::ByteArray(engine.calling_script_hash().to_vec()),
            StackItem::String(url),
            filter.map_or(StackItem::Null, StackItem::String),
        ]));

        Ok(())
    }

    #[contract_method(cpu_fee = 1 << 15)]
    fn verify(&self, engine: &ApplicationEngine) -> bool {
        engine.script_container()
            .as_transaction()
            .and_then(|tx| tx.get_attribute::<OracleResponse>())
            .is_some()
    }
}

#[derive(Serialize, Deserialize)]
struct IdList(HashSet<u64>);

impl IdList {
    fn new() -> Self {
        IdList(HashSet::new())
    }

    fn insert(&mut self, id: u64) {
        self.0.insert(id);
    }

    fn remove(&mut self, id: &u64) -> bool {
        self.0.remove(id)
    }

    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn len(&self) -> usize {
        self.0.len()
    }

    fn iter(&self) -> impl Iterator<Item = &u64> {
        self.0.iter()
    }
}

impl IInteroperable for IdList {
    fn from_stack_item(&mut self, item: StackItem) -> Result<(), String> {
        if let StackItem::Array(array) = item {
            self.0 = array.into_iter()
                .map(|item| item.as_integer().map(|i| i as u64))
                .collect::<Result<HashSet<_>, _>>()?;
            Ok(())
        } else {
            Err("Expected Array".into())
        }
    }

    fn to_stack_item(&self, reference_counter: &mut ReferenceCounter) -> StackItem {
        StackItem::Array(Array::new_with_items(
            self.0.iter().map(|&id| StackItem::Integer(id as i64)).collect(),
            reference_counter,
        ))
    }
}
