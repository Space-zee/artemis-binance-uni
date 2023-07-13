use ethers::{
    prelude::Lazy,
    types::{Address},
};

//pub const Q96: U256 = U256::from(2).pow(U256::from(96));
//pub const Q192: U256 =  U256::from(2).pow(U256::from(192));
pub static POOL_ADDRESS: Lazy<Address> = Lazy::new(|| {
    "0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640"
        .parse()
        .unwrap()
});
