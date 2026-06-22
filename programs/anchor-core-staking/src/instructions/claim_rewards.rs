use crate::error::ErrorCode;
use crate::state::Config;
use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{mint_to_checked, Mint, MintToChecked, TokenAccount, TokenInterface},
};
use mpl_core::{
    accounts::{BaseAssetV1, BaseCollectionV1},
    fetch_plugin,
    instructions::UpdatePluginV1CpiBuilder,
    types::{Attribute, Attributes, Plugin, PluginType, UpdateAuthority},
    ID as MPL_CORE_ID,
};

#[derive(Accounts)]
pub struct ClaimRewards<'info> {
     #[account(mut)]
    pub owner: Signer<'info>,
    #[account(
        seeds = [b"config", collection.key().as_ref()],
        bump = config.bump,
    )]
    pub config: Account<'info, Config>,
    #[account(
        mut,
        has_one = owner @ ErrorCode::InvalidOwner,
        constraint = asset.update_authority == UpdateAuthority::Collection(collection.key()) @ ErrorCode::InvalidUpdateAuthority,
    )]
    pub asset: Account<'info, BaseAssetV1>,
    #[account(
        mut,
        has_one = update_authority @ ErrorCode::InvalidUpdateAuthority
    )]
    pub collection: Account<'info, BaseCollectionV1>,
    /// CHECK: This account data is not used, we only verify the address
    #[account(
        seeds = [b"update_authority", collection.key().as_ref()],
        bump,
    )]
    pub update_authority: UncheckedAccount<'info>,
    #[account(
        mut,
        seeds = [b"rewards_mint", config.key().as_ref()],
        bump = config.rewards_bump,
    )]
    pub rewards_mint: InterfaceAccount<'info, Mint>,
    #[account(
        init_if_needed,
        payer = owner,
        associated_token::mint = rewards_mint,
        associated_token::authority = owner,
    )]
    pub user_rewards_ata: InterfaceAccount<'info, TokenAccount>,
    pub token_program: Interface<'info, TokenInterface>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
    /// CHECK: This is the MPL Core program
    #[account(address = Pubkey::from(MPL_CORE_ID.to_bytes()))]
    pub mpl_core_program: UncheckedAccount<'info>,
}

impl<'info> ClaimRewards<'info> {

pub fn handler(&mut self,ctxbumps:ClaimRewardsBumps) -> Result<()> {

   let attributes_fetched: Option<Attributes> = fetch_plugin::<BaseAssetV1, Attributes>(
        &self.asset.to_account_info(),
        PluginType::Attributes,
    )
    .ok()
    .map(|(_, attrs, _)| attrs);
 let current_timestamp = Clock::get()?.unix_timestamp;

      let attributes = attributes_fetched.unwrap();
  let mut staked_timestamp: i64 = 0;
    let mut last_claimed_timestamp: i64 = 0;
    let mut staked = false;

    let mut attributes_list: Vec<Attribute> = Vec::with_capacity(attributes.attribute_list.len());
    for attribute in &attributes.attribute_list {
        if attribute.key == "staked" {
            require!(attribute.value == "true", ErrorCode::AssetNotStaked);
            staked = true;
            attributes_list.push(attribute.clone());
        } else if attribute.key == "staked_at" {
            staked_timestamp = attribute.value.parse::<i64>().map_err(|_| ErrorCode::InvalidTimestamp)?;
            attributes_list.push(attribute.clone());
        } else if attribute.key == "last_claimed_at" {
            last_claimed_timestamp = attribute.value.parse::<i64>().map_err(|_| ErrorCode::InvalidTimestamp)?;
            // Update this attribute with the current timestamp
            attributes_list.push(Attribute {
                key: "last_claimed_at".to_string(),
                value: current_timestamp.to_string(),
            });
        } else {
            attributes_list.push(attribute.clone());
        }
    }



     if last_claimed_timestamp == 0 {
        last_claimed_timestamp = staked_timestamp;
        // Add the last_claimed_at attribute if it wasn't present
        attributes_list.push(Attribute {
            key: "last_claimed_at".to_string(),
            value: current_timestamp.to_string(),
        });
    }
  let collection_key = self.collection.key();
     let claimable_time = current_timestamp
        .checked_sub(last_claimed_timestamp)
        .ok_or(ErrorCode::InvalidTimestamp)?;
    let claimable_days = claimable_time.checked_div(86400).ok_or(ErrorCode::InvalidTimestamp)?;

  let amount = (claimable_days as u64)
        .checked_mul(self.config.rewards_bps as u64)
        .ok_or(ErrorCode::InvalidRewardsBps)?
        .checked_mul(10u64.pow(self.rewards_mint.decimals as u32))
        .ok_or(ErrorCode::InvalidRewardsBps)?
        .checked_div(10000u64)
        .ok_or(ErrorCode::InvalidRewardsBps)?;

  let signer_seeds = &[
        b"update_authority",
        collection_key.as_ref(),
        &[ctxbumps.update_authority],
    ];

 UpdatePluginV1CpiBuilder::new(&self.mpl_core_program.to_account_info())
        .asset(&self.asset.to_account_info())
        .collection(Some(&self.collection.to_account_info()))
        .payer(&self.owner.to_account_info())
        .authority(Some(&self.update_authority.to_account_info()))
        .system_program(&self.system_program.to_account_info())
        .plugin(Plugin::Attributes(Attributes { attribute_list: attributes_list }))
        .invoke_signed(&[signer_seeds])?;


    let config_seeds = &[
        b"config",
        collection_key.as_ref(),
        &[self.config.bump],
    ];
    let config_signer_seeds = &[&config_seeds[..]];

    mint_to_checked(
        CpiContext::new_with_signer(
            self.token_program.to_account_info(),
            MintToChecked {
                mint: self.rewards_mint.to_account_info(),
                to: self.user_rewards_ata.to_account_info(),
                authority: self.config.to_account_info(),
            },
            config_signer_seeds,
        ),
        amount,
        self.rewards_mint.decimals,
    )?;

    Ok(())

}
}