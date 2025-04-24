//Test file still in review 
import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { assert } from "chai";
import { Stfix } from "../target/types/stfix";
import { Keypair, SystemProgram, PublicKey } from "@solana/web3.js";
import BN from "bn.js";

describe("STFIX", () => {
  // Set provider to local cluster
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.Stfix as Program<Stfix>;

  const configKeypair = Keypair.generate();
  const stfixMint = Keypair.generate();
  const principalVault = Keypair.generate();
  const yieldVault = Keypair.generate();

  it("Initializes the config", async () => {
    const yieldRate30 = new BN(500); // e.g. 5.00% = 500 bps
    const yieldRate90 = new BN(1500); // e.g. 15.00% = 1500 bps
    const cooldown = new BN(3600); // 1 hour
    const penalty = new BN(300); // 3% = 300 bps
    const whitelistOnly = false;

    const [configPda, configBump] = await PublicKey.findProgramAddress(
      [Buffer.from("config")],
      program.programId
    );

    const [vaultPda1, vaultBump1] = await PublicKey.findProgramAddress(
      [Buffer.from("principal-vault")],
      program.programId
    );

    const [vaultPda2, vaultBump2] = await PublicKey.findProgramAddress(
      [Buffer.from("yield-vault")],
      program.programId
    );

    const [mintPda, mintBump] = await PublicKey.findProgramAddress(
      [Buffer.from("stfix-mint")],
      program.programId
    );

    await program.methods
      .initialize(
        yieldRate30,
        yieldRate90,
        cooldown,
        penalty,
        whitelistOnly
      )
      .accounts({
        config: configPda,
        principalVault: vaultPda1,
        yieldVault: vaultPda2,
        stfixMint: mintPda,
        admin: provider.publicKey,
        systemProgram: SystemProgram.programId,
        tokenProgram: anchor.utils.token.TOKEN_PROGRAM_ID,
        rent: anchor.web3.SYSVAR_RENT_PUBKEY,
      })
      .rpc();

    const configAccount = await program.account.config.fetch(configPda);

    console.log("✅ Config initialized:", configAccount);

    assert.equal(configAccount.admin.toBase58(), provider.publicKey.toBase58());
    assert.equal(configAccount.yieldRate30.toNumber(), yieldRate30.toNumber());
    assert.equal(configAccount.yieldRate90.toNumber(), yieldRate90.toNumber());
    assert.equal(configAccount.cooldownSeconds.toNumber(), cooldown.toNumber());
    assert.equal(configAccount.penaltyRateBps.toNumber(), penalty.toNumber());
    assert.equal(configAccount.whitelistOnly, whitelistOnly);
  });
});
