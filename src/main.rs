use std::{net::SocketAddr, str::FromStr};

use axum::{
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use solana_sdk::{signature::{Keypair, Signature}, signer::Signer};
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
};
use spl_token::instruction::{initialize_mint, mint_to};
use base64; // ✅ Needed for encoding instruction data

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", get(check))
        .route("/keypair", post(generate_keypair))
        .route("/token/create", post(create_token))
        .route("/token/mint", post(mint_token))
        .route("/message/sign", post(sign_message))
        .route("/send/sol", post(send_sol))
        .route("/message/verify", post(verify_message));

        // .route("/message/verify", post(verify_message));

    
    let port: u16 = std::env::var("PORT")
    .unwrap_or_else(|_| "3000".to_string())
    .parse()
    .expect("PORT must be a number");

    let addr = SocketAddr::from(([0, 0, 0, 0], port));




    // let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("Failed to bind");

    axum::serve(listener, app).await.expect("Server failed");
}

async fn check() -> &'static str {
    "Hello World"
}

#[derive(Serialize)]
#[serde(untagged)]
enum ApiResponse<T> {
    Success { success: bool, data: T },
    Error { success: bool, error: String },
}

impl<T> From<Result<T, String>> for ApiResponse<T> {
    fn from(result: Result<T, String>) -> Self {
        match result {
            Ok(data) => ApiResponse::Success {
                success: true,
                data,
            },
            Err(error) => ApiResponse::Error {
                success: false,
                error,
            },
        }
    }
}

#[derive(Serialize)]
struct KeypairResponse {
    pubkey: String,
    secret: String,
}

async fn generate_keypair() -> impl IntoResponse {
    let keypair = Keypair::new();
    let pubkey = keypair.pubkey().to_string();
    let secret_bytes = keypair.to_bytes();
    let secret = bs58::encode(secret_bytes).into_string();

    let response = KeypairResponse { pubkey, secret };

    Json(ApiResponse::from(Ok(response)))
}

#[derive(Debug, Deserialize)]
struct TokenCreateRequest {
    mint_authority: String,
    mint: String,
    decimals: u8,
}

#[derive(Serialize)]
struct TokenCreateResponse {
    program_id: String,
    accounts: Vec<AccountMetaJson>,
    instruction_data: String,
}

#[derive(Serialize)]
struct AccountMetaJson {
    pubkey: String,
    is_signer: bool,
    is_writable: bool,
}

async fn create_token(Json(req): Json<TokenCreateRequest>) -> impl IntoResponse {
    let mint = match Pubkey::from_str(&req.mint) {
        Ok(p) => p,
        Err(_) => return Json(ApiResponse::from(Err("Invalid mint pubkey".into()))),
    };

    let mint_authority = match Pubkey::from_str(&req.mint_authority) {
        Ok(p) => p,
        Err(_) => return Json(ApiResponse::from(Err("Invalid mint_authority pubkey".into()))),
    };

    let freeze_authority = None;

    let ix = initialize_mint(
        &spl_token::id(),
        &mint,
        &mint_authority,
        freeze_authority.as_ref(),
        req.decimals,
    );

    if let Err(e) = ix {
        return Json(ApiResponse::from(Err(format!("Failed to build instruction: {e}"))));
    }

    let instruction = ix.unwrap();

    let accounts: Vec<AccountMetaJson> = instruction
        .accounts
        .iter()
        .map(|meta| AccountMetaJson {
            pubkey: meta.pubkey.to_string(),
            is_signer: meta.is_signer,
            is_writable: meta.is_writable,
        })
        .collect();

    let instruction_data = base64::encode(instruction.data.clone());

    let response = TokenCreateResponse {
        program_id: instruction.program_id.to_string(),
        accounts,
        instruction_data,
    };

    Json(ApiResponse::from(Ok(response)))
}

#[derive(Debug, Deserialize)]
struct MintTokenRequest {
    mint: String,
    destination: String,
    authority: String,
    amount: u64,
}



async fn mint_token(Json(req): Json<MintTokenRequest>) -> impl IntoResponse {
    let mint = match Pubkey::from_str(&req.mint) {
        Ok(p) => p,
        Err(_) => return Json(ApiResponse::from(Err("Invalid mint pubkey".into()))),
    };

    let destination = match Pubkey::from_str(&req.destination) {
        Ok(p) => p,
        Err(_) => return Json(ApiResponse::from(Err("Invalid destination pubkey".into()))),
    };

    let authority = match Pubkey::from_str(&req.authority) {
        Ok(p) => p,
        Err(_) => return Json(ApiResponse::from(Err("Invalid authority pubkey".into()))),
    };

    let instruction_result = mint_to(
        &spl_token::id(),
        &mint,
        &destination,
        &authority,
        &[], // no multisig signers
        req.amount,
    );

    if let Err(e) = instruction_result {
        return Json(ApiResponse::from(Err(format!("Failed to build mint instruction: {}", e))));
    }

    let instruction = instruction_result.unwrap();

    let accounts: Vec<AccountMetaJson> = instruction
        .accounts
        .iter()
        .map(|meta| AccountMetaJson {
            pubkey: meta.pubkey.to_string(),
            is_signer: meta.is_signer,
            is_writable: meta.is_writable,
        })
        .collect();

    let response = TokenCreateResponse {
        program_id: instruction.program_id.to_string(),
        accounts,
        instruction_data: base64::encode(instruction.data.clone()),
    };

    Json(ApiResponse::from(Ok(response)))
}

#[derive(Debug, Deserialize)]
struct SignMessageRequest {
    message: String,
    secret: String,
}

#[derive(Serialize)]
struct SignMessageResponse {
    signature: String,
    public_key: String,
    message: String,
}

async fn sign_message(Json(req): Json<SignMessageRequest>) -> impl IntoResponse {
    if req.message.trim().is_empty() || req.secret.trim().is_empty() {
        return Json(ApiResponse::<SignMessageResponse>::Error {
            success: false,
            error: "Missing required fields".to_string(),
        });
    }

    // Decode base58-encoded secret key
    let secret_bytes = match bs58::decode(&req.secret).into_vec() {
        Ok(bytes) if bytes.len() == 64 => bytes,
        _ => {
            return Json(ApiResponse::<SignMessageResponse>::Error {
                success: false,
                error: "Invalid secret key".to_string(),
            });
        }
    };

    // Create Keypair from bytes
    let keypair = match Keypair::from_bytes(&secret_bytes) {
        Ok(kp) => kp,
        Err(_) => {
            return Json(ApiResponse::<SignMessageResponse>::Error {
                success: false,
                error: "Failed to construct keypair".to_string(),
            });
        }
    };

    // Sign message
    let signature = keypair.sign_message(req.message.as_bytes());

    let response = SignMessageResponse {
        signature: base64::encode(signature.as_ref()),
        public_key: keypair.pubkey().to_string(),
        message: req.message.clone(),
    };

    Json(ApiResponse::from(Ok(response)))
}



use solana_sdk::system_instruction::transfer;
use solana_sdk::system_program;

#[derive(Debug, Deserialize)]
struct SendSolRequest {
    from: String,
    to: String,
    lamports: u64,
}

#[derive(Serialize)]
struct SendSolResponse {
    program_id: String,
    accounts: Vec<String>,
    instruction_data: String,
}

async fn send_sol(Json(req): Json<SendSolRequest>) -> impl IntoResponse {
    // Validate addresses
    let from = match Pubkey::from_str(&req.from) {
        Ok(p) => p,
        Err(_) => return Json(ApiResponse::from(Err("Invalid sender pubkey".into()))),
    };

    let to = match Pubkey::from_str(&req.to) {
        Ok(p) => p,
        Err(_) => return Json(ApiResponse::from(Err("Invalid recipient pubkey".into()))),
    };

    // Create instruction
    let instruction = transfer(&from, &to, req.lamports);

    // Serialize accounts and instruction data
    let accounts: Vec<String> = instruction
        .accounts
        .iter()
        .map(|meta| meta.pubkey.to_string())
        .collect();

    let instruction_data = base64::encode(instruction.data.clone());

    let response = SendSolResponse {
        program_id: instruction.program_id.to_string(),
        accounts,
        instruction_data,
    };

    Json(ApiResponse::from(Ok(response)))
}




// Add this struct for the verify request
#[derive(Debug, Deserialize)]
struct VerifyMessageRequest {
    message: String,
    signature: String, // base64-encoded signature
    pubkey: String,    // base58-encoded public key
}

// Add this struct for the verify response
#[derive(Serialize)]
struct VerifyMessageResponse {
    valid: bool,
    message: String,
    pubkey: String,
}

// Add this function to handle message verification
async fn verify_message(Json(req): Json<VerifyMessageRequest>) -> impl IntoResponse {
    // Validate input fields
    if req.message.trim().is_empty() || req.signature.trim().is_empty() || req.pubkey.trim().is_empty() {
        return Json(ApiResponse::<VerifyMessageResponse>::Error {
            success: false,
            error: "Missing required fields".to_string(),
        });
    }

    // Parse the public key
    let pubkey = match Pubkey::from_str(&req.pubkey) {
        Ok(pk) => pk,
        Err(_) => {
            return Json(ApiResponse::<VerifyMessageResponse>::Error {
                success: false,
                error: "Invalid public key".to_string(),
            });
        }
    };

    // Decode the base64-encoded signature
    let signature_bytes = match base64::decode(&req.signature) {
        Ok(bytes) if bytes.len() == 64 => bytes,
        _ => {
            return Json(ApiResponse::<VerifyMessageResponse>::Error {
                success: false,
                error: "Invalid signature format".to_string(),
            });
        }
    };

    // Create signature object
    let signature = match Signature::try_from(signature_bytes.as_slice()) {
        Ok(sig) => sig,
        Err(_) => {
            return Json(ApiResponse::<VerifyMessageResponse>::Error {
                success: false,
                error: "Failed to parse signature".to_string(),
            });
        }
    };

    // Verify the signature
    let is_valid = signature.verify(&pubkey.to_bytes(), req.message.as_bytes());

    let response = VerifyMessageResponse {
        valid: is_valid,
        message: req.message.clone(),
        pubkey: req.pubkey.clone(),
    };

    Json(ApiResponse::from(Ok(response)))
}

// Don't forget to add the route in your main function:
// .route("/message/verify", post(verify_message));

