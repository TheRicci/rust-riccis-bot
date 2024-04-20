mod commands;
mod coingecko;
mod twelvedata;

use coingecko::CoinsListItem;
use rand::Rng;
use dotenv;
use log::{info, error,debug};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::{env, vec};
use std::sync::Arc;
use serenity::model::application::interaction::{Interaction, InteractionResponseType,MessageFlags};
use std::{time,thread,time::Duration};
use thousands::Separable;
use serenity::model::application::command::Command;
use serenity::async_trait;
use serenity::client::bridge::gateway::{ShardId, ShardManager};
use serenity::model::gateway::Activity;
use serenity::framework::standard::buckets::{LimitedFor};
use serenity::framework::standard::macros::{check, command, group, help, hook};
use serenity::framework::standard::{
    help_commands,
    Args,
    CommandGroup,
    CommandResult,
    CommandOptions,
    CommandError,
    DispatchError,
    HelpOptions,
    Reason,
    StandardFramework,
};
use serenity::model::id::CommandId;

use std::fs::{File, OpenOptions};
use url::Url;
use tungstenite::connect;
use tungstenite::Message as TunMessage;
use serenity::http::Http;
use serenity::model::channel::{Channel, Message};
use serenity::model::gateway::{GatewayIntents, Ready};
use serenity::model::id::UserId;
use serenity::prelude::*;
use tokio::sync::{Mutex,RwLock};

use redis::{Commands, JsonCommands, ErrorKind};

struct ShardManagerContainer;

impl TypeMapKey for ShardManagerContainer {
    type Value = Arc<Mutex<ShardManager>>;
}

struct GeckoClient;
impl TypeMapKey for GeckoClient {
    type Value = Arc<coingecko::CoinGeckoClient>;
}

struct StonksClient;
impl TypeMapKey for StonksClient {
    type Value = Arc<twelvedata::TwelveDataClient>;
}

struct RedisClient;
impl TypeMapKey for RedisClient {
    type Value = Arc<redis::Client>;
}

struct CountriesMap;
impl TypeMapKey for CountriesMap {
    type Value = Arc<HashMap<String, HashSet<String>>>;
}

struct CoingeckoIDMap;
impl TypeMapKey for CoingeckoIDMap {
    type Value = Arc<RwLock<HashMap<String, String>>>;
}

struct SymbolCoingeckoIDMap;
impl TypeMapKey for SymbolCoingeckoIDMap {
    type Value = Arc<RwLock<HashMap<String, String>>>;
}

struct NameCoingeckoIDMap;
impl TypeMapKey for NameCoingeckoIDMap {
    type Value = Arc<RwLock<HashMap<String, String>>>;
}

struct Handler;

#[async_trait]
impl EventHandler for Handler {
    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Interaction::ApplicationCommand(command) = interaction {
            info!("Received command interaction: {}", &command.data.name);

            let content = match command.data.name.as_str() {
                "feedback" => commands::feedback::run(&command.data.options),
                _ => "not implemented :(".to_string(),
            };

            if let Err(why) = command
                .create_interaction_response(&ctx.http, |response| {
                    response
                        .kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|message| message.content(content).flags(MessageFlags::EPHEMERAL))
                })
                .await
            {
                error!("Cannot respond to slash command: {}", why);
            }
        }
    }
    async fn ready(&self, ctx: Context, ready: Ready) {
        info!("{} is connected!", ready.user.name);

        let command = Command::create_global_application_command(&ctx.http, |command| {
            commands::feedback::register(command)
        })
        .await;

        info!("following global command was created: {:#?}", command.unwrap().name);

        let ctx1 = Arc::clone(& Arc::new(ctx));
        // tokio::spawn creates a new green thread that can run in parallel with the rest of
        // the application.
        tokio::spawn(async move {
            loop{
                let (mut socket, _) = connect(
                    Url::parse("wss://stream.binance.com:9443/ws/btcusdt@kline_1s").unwrap()
                ).expect("Can't connect to binance.");
                
                let (mut timer1, mut timer2) = (time::Instant::now(), time::Instant::now());
                let mut coin:Option<coingecko::CoinsItem> = None;
    
                loop {
                    let msg:tungstenite::protocol::Message;
                    match socket.read_message(){
                        Ok(m) => msg = m,
                        Err(_) => break,
                    }
                
                    match msg {
                        TunMessage::Text(t) =>{
                            let parsed: serde_json::Value = serde_json::from_str(&t).expect("Can't parse to JSON");
                            if time::Instant::now().duration_since(timer1).as_secs() < 20{
                                continue;
                            }
                            let context_data_map = ctx1.data.read().await;

                            let redis_client =  match context_data_map.get::<RedisClient>(){
                                Some(r) => r.clone(),
                                None => {error!("there was a problem getting redis client."); continue} 
                            };
                            let mut con = match redis_client.get_connection(){
                                Ok(c) => c,
                                Err(y) => {error!("{y}"); continue}
                            };                               
                            
                            timer1 = time::Instant::now();
                            if time::Instant::now().duration_since(timer2).as_secs() > 240 || coin.is_none(){
                                let coingecko_client = match context_data_map.get::<GeckoClient>(){
                                    Some(c) => c.clone(),
                                    None => {error!("failed to get coingecko client object"); continue}
                                };
                                match coingecko_client.coin("bitcoin", false, false, true, false, false, false).await{
                                    Ok(c) => {
                                        match con.json_set(format!("crypto:bitcoin"), "$",&c){
                                            Ok(()) => match con.expire(format!("crypto:bitcoin"), 300) {
                                                Ok(()) =>  debug!("stored new cache for bitcoin."),
                                                Err(y) => {error!("{y}"); continue}
                                            },
                                            Err(y) => {error!("{y}"); continue}
                                        }
                                        coin = Some(c);
                                    }
                                    Err(y) =>{error!("error while requesting coingecko: {y}"); continue}
                                }

                            timer2 = time::Instant::now();
                            }else{
                                let coin_redis_resp:Option<String> = match con.json_get(format!("crypto:bitcoin"), "$"){
                                    Ok(c) => c,
                                    Err(y) => if let ErrorKind::TypeError = y.kind() {
                                        None
                                    }else{error!("{}",y.to_string().to_lowercase()); continue}
                                };
                        
                                match coin_redis_resp {
                                    Some(c) =>{
                                        let c:Vec<coingecko::CoinsItem> = serde_json::from_str(&c).unwrap();
                                        coin = Some(c[0].clone());
                                        debug!("used cache for bitcoin.")
                                    }None =>{error!("bitcoin coingecko data not found on redis."); continue}
                                }
                            }
                        
                            let one_day =  match &coin {
                                Some(c) => match &c.market_data{
                                    Some(d) => match &d.price_change_percentage24_h_in_currency{
                                        Some(currency) =>match currency.usd {
                                            Some(f) => f,
                                            None => {error!("no usd for one day data"); continue},
                                        }
                                        None => {error!("no one day change data"); continue},
                                    }
                                    None => {error!("no bitcoin market data stored"); continue},
                                }
                                None => {error!("no bitcoin data stored"); continue},
                            };
                            
                            let price = parsed["k"]["c"].as_str().unwrap();
                            ctx1.set_activity(Activity::watching(format!("$ {} {} {}%",format_price(price.trim().parse().unwrap()).separate_with_commas(),check_direction(&one_day,true),add_sign(one_day)))).await;
                            if one_day.is_sign_positive(){ctx1.online().await}else{ctx1.dnd().await}
                            }
                        
                        TunMessage::Ping(t) => {
                            socket.write_message(TunMessage::Pong(t)).expect("failed to pong");
                        }
                        _ => break
                    }    
                }
            }
        });

    }
}

#[group]
#[only_in(guilds)]
#[commands(about,delete,stonk_price)]
struct General;

#[group]
#[only_in(guilds)]
#[commands(crypto_price,index)]
struct Crypto;

#[group]
#[owners_only]
#[only_in(guilds)]
#[summary = "Commands for server owners"]
#[commands(slow_mode,latency)]
struct Owner;

#[help]
#[individual_command_tip = "Hello! Olá! こんにちは！Hola! Bonjour! 您好! 안녕하세요~\n
If you want more information about a specific command,
just pass the command as argument. \n
Commands available:\n"]
#[strikethrough_commands_tip_in_guild("")]
#[max_levenshtein_distance(3)]
#[indention_prefix = "+"]
#[lacking_permissions = "Hide"]
async fn my_help(
    context: &Context,
    msg: &Message,
    args: Args,
    help_options: &'static HelpOptions,
    groups: &[&'static CommandGroup],
    owners: HashSet<UserId>,
) -> CommandResult {
    let _ = help_commands::with_embeds(context, msg, args, help_options, groups, owners).await;
    Ok(())
}

#[hook]
async fn before(ctx: &Context, msg: &Message, _command_name: &str) -> bool {
    let guild_name = match msg.guild(ctx){
        Some(n) => n.name,
        None => "null".to_string(),
    };
    info!("Got command '{}', by user '{}, from: {}'",msg.content,msg.author.name,guild_name);

    true
}

#[hook]
async fn after(ctx: &Context, msg: &Message, _command_name: &str, command_result: CommandResult) {
    let guild_name = match msg.guild(ctx){
        Some(n) => n.name,
        None => "null".to_string(),
    };
    match command_result {
        Ok(()) => info!("Command '{}', by user '{}, from: {} processed succesfuly.'",msg.content,msg.author.name,guild_name),
        Err(why) =>{
            let w = why.to_string();
            error!("Command '{}', by user '{}, from: {} had an error: {}",msg.content,msg.author.name,guild_name,w);
            if w.chars().next().unwrap().is_uppercase(){
                let _ = msg.reply(ctx,w).await;
            }else{
                let _ = msg.reply(ctx,"Internal Error.").await;
            }
        } 
    }
}

#[hook]
async fn delay_action(ctx: &Context, msg: &Message) {
    let _ = msg.react(ctx, '⏱').await;
}

#[hook]
async fn dispatch_error(ctx: &Context, msg: &Message, error: DispatchError, _command_name: &str) {
    if let DispatchError::Ratelimited(info) = error {
        let _ = msg.delete(ctx).await;
        let text:String;
        if info.as_secs() > 60{
            text = format!("{} seconds.", info.as_secs())
        }else{
            text = format!("{:.2} minutes.", info.as_secs()/60)
        }
        let msg = msg.channel_id.say(&ctx.http, &format!("Try this again in {text}")).await;

        if let Ok(msg) = msg {
            thread::sleep(time::Duration::from_secs(4));
            let _ = msg.delete(ctx).await;
        }

    }
}

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();
    env_logger::init();
    let coingecko_client = coingecko::CoinGeckoClient::new("https://api.coingecko.com/api/v3");

    let mut coins_map_id: HashMap<String,String> = HashMap::default();
    let mut coins_map_name: HashMap<String,String> = HashMap::default();
    let mut coins_map_symbol: HashMap<String,String> = HashMap::default();
    
    let mut file = OpenOptions::new().read(true).open("coins.json").unwrap();
    let mut json_string = String::new();
    file.read_to_string(&mut json_string).unwrap();
    
    let deserialized_maps: Result<Vec<HashMap<String, String>>, _> = serde_json::from_str(&json_string);
    match deserialized_maps {
        Ok(maps) => {
            coins_map_id = maps[0].clone();
            coins_map_name = maps[1].clone();
            coins_map_symbol = maps[2].clone();
        }
        Err(e) => {
            println!("Error deserializing JSON: {}", e);
        }
    }
    
    let arc_id = Arc::new(RwLock::new(coins_map_id));
    let arc_name = Arc::new(RwLock::new(coins_map_name));
    let arc_symbol = Arc::new(RwLock::new(coins_map_symbol));

    let (map_id1,map_name1,map_symbol1) = (arc_id.clone(),arc_name.clone(),arc_symbol.clone()); 
    tokio::spawn(async move {
        let mut i = 1;
        loop {
            match coingecko_client.coins_markets(i).await{
                Ok(res) =>{
                    if res.is_empty() {
                        break;
                    }
                    { // code block to drop write locks
                        let (mut id_write,mut name_write,mut symbol_write) = (map_id1.write().await,map_name1.write().await,map_symbol1.write().await);
                        for data in res {
                            id_write.insert(data.id.clone(), data.id.clone());
                            name_write.insert(data.name, data.id.clone());
                            symbol_write.insert(data.symbol, data.id);
                        }
                        let list:Vec<HashMap<String,String>> = vec![id_write.clone(),name_write.clone(),symbol_write.clone()];
                        let json_string = serde_json::to_string(&list).unwrap();
                        let mut file = OpenOptions::new().write(true).open("coins.json").unwrap();
                        let _ = file.write_all(json_string.as_bytes());
                        
                    }
                    thread::sleep(Duration::from_secs(60));
            
                    i+=1;
                }
                Err(y) => {println!("{:?}",y);thread::sleep(Duration::from_secs(60));continue}
            };
           
        }
    });

    let token = env::var("DISCORD_TOKEN").expect("Expected a token in the environment");
    let http = Http::new(&token);
    
    let (owners, bot_id) = match http.get_current_application_info().await {
        Ok(info) => {
            let mut owners = HashSet::new();
            if let Some(team) = info.team {
                owners.insert(team.owner_user_id);
            } else {
                owners.insert(info.owner.id);
            }
            match http.get_current_user().await {
                Ok(bot_id) => (owners, bot_id.id),
                Err(why) => panic!("Could not access the bot id: {:?}", why),
            }
        },
        Err(why) => panic!("Could not access application info: {:?}", why),
    };

    let framework = StandardFramework::new()
        .configure(|c| c
                   .with_whitespace(true)
                   .on_mention(Some(bot_id))
                   .prefix(".")
                   .delimiters(vec![", ", ","," ","/"])
                   .owners(owners))
                    
        .before(before)
        .after(after)
        .on_dispatch_error(dispatch_error)
        .bucket("price", |b| b.limit(1).time_span(20).delay(0)
            .limit_for(LimitedFor::Channel)
            .await_ratelimits(1)
            .delay_action(delay_action)).await
        .bucket("index", |b| b.delay(3600).limit_for(LimitedFor::Channel)).await
        .help(&MY_HELP)
        .group(&GENERAL_GROUP)
        .group(&CRYPTO_GROUP)
        .group(&OWNER_GROUP);

    let intents = GatewayIntents::all();
    let mut client = Client::builder(&token, intents)
        .event_handler(Handler)
        .framework(framework)
        .await
        .expect("Err creating client");
    
    let redis_client = redis::Client::open("redis://127.0.0.1/").expect("failed to connect to connect to Redis");
    
    let twelvedata_client = twelvedata::TwelveDataClient::new("https://twelve-data1.p.rapidapi.com");
    let stocks = twelvedata_client.stocks_list().await.expect("failed to fetch stocks list");
    let indices = twelvedata_client.indices_list().await.expect("failed to fetch indices list");
    let etfs = twelvedata_client.etfs_list().await.expect("failed to fetch etfs list");
    let mut symbols_map: HashMap<String,HashSet<String>> = HashMap::default();

    for stock in stocks.data{
        let map = symbols_map.entry(stock.country.to_lowercase()).or_insert(HashSet::default());
        map.insert(stock.symbol.to_lowercase());

    }
    for i in indices.data{
        let map = symbols_map.entry(i.country.to_lowercase()).or_insert(HashSet::default());
        map.insert(i.symbol.to_lowercase());
    }
    for etf in etfs.data{
        let map = symbols_map.entry(etf.country.to_lowercase()).or_insert(HashSet::default());
        map.insert(etf.symbol.to_lowercase());

    }
    
    {
        let mut data = client.data.write().await;
        data.insert::<ShardManagerContainer>(Arc::clone(&client.shard_manager));
        data.insert::<GeckoClient>(Arc::new(coingecko_client));
        data.insert::<CoingeckoIDMap>(arc_id.clone());
        data.insert::<SymbolCoingeckoIDMap>(arc_symbol.clone());
        data.insert::<NameCoingeckoIDMap>(arc_name.clone());
        data.insert::<StonksClient>(Arc::new(twelvedata_client));
        data.insert::<CountriesMap>(Arc::new(symbols_map));
        data.insert::<RedisClient>(Arc::new(redis_client))

    }

    if let Err(why) = client.start().await {
        error!("Client error: {:?}", why);
    }

}

#[command]
async fn about(ctx: &Context, msg: &Message) -> CommandResult {
    let _ = msg.channel_id.say(&ctx.http, "currently a crypto related bot :)").await;

    Ok(())
}

#[command]
#[aliases("i")]
#[bucket = "index" ]
#[description = "pulls fear and greed index for bitcoin."]
#[example = ".i"]
#[example = ".index"]
async fn index(ctx: &Context, msg: &Message) -> CommandResult {
    let rng = rand::thread_rng().gen_range(0..100);
    let _ = msg.channel_id.send_message(&ctx.http,
    |c| c.add_embed(
        |e|e.image(format!("https://alternative.me/crypto/fear-and-greed-index.png?{rng}"))
    )).await;

    Ok(())
}

#[check]
#[name = "Owner"]
async fn owner_check(
    _: &Context,
    msg: &Message,
    _: &mut Args,
    _: &CommandOptions,
) -> Result<(), Reason> {
    if msg.author.id != 260444527779643402{
        return Err(Reason::User("Lacked owner permission".to_string()));
    }

    Ok(())
}

#[command]
#[checks(Owner)]
#[help_available(false)]
async fn delete(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    let id = match args.single::<u64>(){
        Ok(id) => id,
        Err(y) => return Err(CommandError::from(format!("error while parsing command: {y}")))
    };

    match Command::delete_global_application_command(ctx, CommandId(id)).await{
        Ok(()) => {
            let _ = msg.reply(ctx, format!("command {id} deleted succesfuly.")).await;
        }
        Err(y) =>return Err(CommandError::from(format!("error while deleting command: {y}")))
    }

    Ok(())
}

#[command]
#[required_permissions("ADMINISTRATOR")]
async fn latency(ctx: &Context, msg: &Message) -> CommandResult {
    // The shard manager is an interface for mutating, stopping, restarting, and
    // retrieving information about shards.
    let data = ctx.data.read().await;

    let shard_manager = match data.get::<ShardManagerContainer>() {
        Some(v) => v,
        None => return Err(CommandError::from("There was a problem getting the shard manager"))
    };

    let manager = shard_manager.lock().await;
    let runners = manager.runners.lock().await;

    // Shards are backed by a "shard runner" responsible for processing events
    // over the shard, so we'll get the information about the shard runner for
    // the shard this command was sent over.
    let runner = match runners.get(&ShardId(ctx.shard_id)) {
        Some(runner) => runner,
        None => return Err(CommandError::from("No shard found"))
    };

    msg.reply(ctx, &format!("The shard latency is {:?}", runner.latency)).await?;

    Ok(())
}

#[command]
#[required_permissions("ADMINISTRATOR")]
#[description = "enables slow mode on current channel."]
#[aliases("slow")]
#[example =".slow_mode"]
#[example = ".slow"]
#[example = ".slow 10"]
async fn slow_mode(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    if let Ok(slow_mode_rate_seconds) = args.single::<u64>() {
        if let Err(why) = msg.channel_id.edit(&ctx.http, |c| c.rate_limit_per_user(slow_mode_rate_seconds)).await{
            return Err(CommandError::from(format!("Failed to set slow mode to `{slow_mode_rate_seconds}` seconds. : {:?}--Failed to set slow mode to `{slow_mode_rate_seconds}` seconds.",why)));
        } else {
            msg.channel_id.say(&ctx.http,format!("Successfully set slow mode rate to `{}` seconds.", slow_mode_rate_seconds)).await?;
        }
    } else if let Some(Channel::Guild(channel)) = msg.channel_id.to_channel_cached(&ctx.cache) {
        let slow_mode_rate = channel.rate_limit_per_user.unwrap_or(0);
        msg.channel_id.say(&ctx.http,format!("Current slow mode rate is `{}` seconds.", slow_mode_rate)).await?;
    } else {
        return Err(CommandError::from("failed to find channel in cache."));
    };

    Ok(())
}


#[command]
#[aliases("p")]
#[bucket = "price" ]
#[description = "pulls crypto coins data from coingecko."]
#[example = ".price bitcoin"]
#[example = ".p bitcoin"]
#[example = ".p btc"]
#[example = ".p btc/eur"]
async fn crypto_price(ctx: &Context,msg: &Message,mut args: Args) -> CommandResult {
    let arg1 = match args.single::<String>(){
        Ok(str) => str,
        Err(_) => "bitcoin".to_string()
    };
    let arg2 = match args.single::<String>(){
        Ok(str) => match str.as_str() {
            "btc" | "eth" | "usd" | "eur" | "brl" => str.to_string(),
            _ => return Err(CommandError::from("Quote currency not supported."))
        }
        Err(_) => "usd".to_string()
    };
    
    let context_data = ctx.data.read().await;
    let coin_id:String;
    {   
        let id_map =  match context_data.get::<CoingeckoIDMap>(){
            Some(m) => m.clone(),
            None => {
              return  Err(CommandError::from("there was a problem fetching the ids hashmap."))
        }};

        let id_map_read = id_map.read().await;
        coin_id = match id_map_read.get(&arg1) {
            Some(v) => v.clone(),
            None => {
                drop(id_map_read);
                let name_map = match context_data.get::<NameCoingeckoIDMap>(){
                    Some(m) => m.clone(),
                    None => {
                        return Err(CommandError::from("there was a problem fetching the name hashmap."))
                }};
                let name_map_read = name_map.read().await;
                let coin_id = match name_map_read.get(&arg1) {
                    Some(v) => v.clone(),
                    None => {
                        drop(name_map_read);
                        let symbol_map = match context_data.get::<SymbolCoingeckoIDMap>(){
                            Some(m) => m,
                            None => 
                                return Err(CommandError::from("there was a problem fetching the symbols hashmap.")) 
                            };
                        let symbol_map_read = symbol_map.read().await;                    
                        let coin_id = match symbol_map_read.get(&arg1) {
                            Some(v) => v.clone(),
                            None => return Err(CommandError::from("Coin not found."))
                            };
                        coin_id.clone()
                    }
                };
                coin_id.clone()
        }};
    }
    let coin:coingecko::CoinsItem;
    {
        let redis_client =  match context_data.get::<RedisClient>(){
            Some(r) => r.clone(),
            None => return Err(CommandError::from("there was a problem getting redis client."))
        };
        let mut con = match redis_client.get_connection(){
            Ok(c) => c,
            Err(y) => return Err(CommandError::from(format!("{}",y.to_string().to_lowercase())))
        };

        let coin_redis_resp:Option<String> = match con.json_get(format!("crypto:{coin_id}:{arg2}"), "$"){
            Ok(c) => c,
            Err(y) => if let ErrorKind::TypeError = y.kind() {
                None
            }else{
                return Err(CommandError::from(format!("{}",y.to_string().to_lowercase())))
            }
        };

        match coin_redis_resp {
            Some(c) =>{
                let c:Vec<coingecko::CoinsItem> = serde_json::from_str(&c)?;
                coin = c[0].clone();
                debug!("used cache for {coin_id}.")
            }None =>{
                let coingecko_client = match context_data.get::<GeckoClient>(){
                    Some(c) => c.clone(),
                    None => return Err(CommandError::from("aoushioausbd"))
                };
                match coingecko_client.coin(coin_id.as_str(), false, false, true, false, false, false).await{
                    Ok(c) => {
                        coin = c.clone();
                        match con.json_set(format!("crypto:{coin_id}:{arg2}"), "$",&c){
                            Ok(()) => match con.expire(format!("crypto:{coin_id}:{arg2}"), 300) {
                                Ok(()) =>  debug!("stored new cache for {coin_id}."),
                                Err(y) => error!("{y}")
                            },
                            Err(y) => error!("{y}")
                        }
                    }
                    Err(y) => return Err(CommandError::from(format!("{}",y.to_string().to_lowercase())))
                }
            }
        }        
    }
    drop(context_data);
      
   let market_data = match coin.market_data{
    Some(data) => data,
    None => return Err(CommandError::from("error while fetching market data."))
    };
   
    let price = match market_data.current_price.gets(&arg2){
        Ok(n) => match n{
            Some(d) => d.clone(),
            None => return Err(CommandError::from("Quote currency not supported."))
        },
        Err(y) => return Err(CommandError::from(format!("{}",y.to_string().to_lowercase())))
    };

    let ath = match market_data.ath.gets(&arg2){
        Ok(n) => match n{
            Some(d) => d.clone(),
            None => return Err(CommandError::from("Quote currency not supported."))
        },
        Err(y) => return Err(CommandError::from(format!("{}",y.to_string().to_lowercase())))
    };

    let atl = match market_data.atl.gets(&arg2){
        Ok(n) => match n{
            Some(d) => d.clone(),
            None => return Err(CommandError::from("Quote currency not supported."))
        },
        Err(y) => return Err(CommandError::from(format!("{}",y.to_string().to_lowercase())))
    };

    let one_hour = match market_data.price_change_percentage1_h_in_currency{
        Some(change) => match change.gets(&arg2){
            Ok(n) => match n{
                Some(d) => d.clone(),
                None => return Err(CommandError::from("Quote currency not supported."))
                },
            Err(y) => return Err(CommandError::from(format!("{}",y.to_string().to_lowercase()))),
            }
        None => return Err(CommandError::from("Quote currency not supported."))
    };

    let one_day = match market_data.price_change_percentage24_h_in_currency{
        Some(change) => match change.gets(&arg2){
            Ok(n) => match n{
                Some(d) => d.clone(),
                None => return Err(CommandError::from("Quote currency not supported."))
                },
            Err(y) => return Err(CommandError::from(format!("{}",y.to_string().to_lowercase()))),
            }
        None => return Err(CommandError::from("Quote currency not supported."))
    };

    let image = match coin.image.thumb{
        Some(data)=> data,
        None => return Err(CommandError::from("failed to fetch image."))
    };
   
    msg.channel_id.send_message(ctx, |c|
        c.add_embed(|e|
            e.thumbnail(image).description(format!(
            "** ​​  ​​ {} ({})** #{}\n\n** ​​ {}  ​​  {} {}**\n```24h: {}%\n1h:  {}%```",
            coin.name, coin.symbol, market_data.market_cap_rank,check_direction(&one_day,false),format_price(price).separate_with_commas(),
            arg2.to_uppercase(),add_sign(one_day),add_sign(one_hour))
        ).footer(|f|f.text(format!("ATH: {} ​​  ATL: {}",format_price(ath),format_price(atl)))))
        ).await?;
    
   Ok(())
}


#[command]
#[aliases("s")]
#[bucket = "price" ]
#[description = "pulls stonk price from twelvedata."]
#[example = ".s amzn"]
#[example = ".s gld"]
async fn stonk_price(ctx: &Context,msg: &Message,mut args: Args) -> CommandResult {
    let stonk_symbol = match args.single::<String>(){
        Ok(str) => str,
        Err(_) => "SPX".to_string()
    };
    let country = match args.remains(){
        Some(str) => {if str.to_lowercase() != "united states" {
            return Err(CommandError::from("Only United States market is available for now."))
        }else{
            str.to_owned().to_lowercase()
        }},
        None => "united states".to_owned()
    };

    let context_data = ctx.data.read().await;
    {   
        let countries_map =  match context_data.get::<CountriesMap>(){
            Some(m) => m.clone(),
            None => return Err(CommandError::from("there was a problem fetching the exchanges hashmap."))
        };

        let stonks_map = match countries_map.get(&country){
            Some(m) => m,
            None => return Err(CommandError::from("Country not found."))
        };

        if !stonks_map.contains(&stonk_symbol.to_lowercase()){
            return Err(CommandError::from("Stock not found."))
        };
    }

    let stonk:twelvedata::Stock;
    {
        let redis_client =  match context_data.get::<RedisClient>(){
            Some(r) => r.clone(),
            None => return Err(CommandError::from("there was a problem getting redis client."))
        };
        let mut con = match redis_client.get_connection(){
            Ok(c) => c,
            Err(y) => return Err(CommandError::from(format!("{}",y.to_string().to_lowercase())))
        };

        let coin_redis_resp:Option<String> = match con.json_get(format!("stock:{stonk_symbol}:{country}"), "$"){
            Ok(c) => c,
            Err(y) => if let ErrorKind::TypeError = y.kind() {
                None
            }else{
                return Err(CommandError::from(format!("{}",y.to_string().to_lowercase())))
            }
        };

        match coin_redis_resp {
            Some(s) =>{
                let s:Vec<twelvedata::Stock> = serde_json::from_str(&s)?;
                stonk = s[0].clone();
                debug!("used cache for {stonk_symbol}:{country}.")
            }None =>{
                let stonk_client = match context_data.get::<StonksClient>(){
                   Some(c) => c.clone(),
                   None => return Err(CommandError::from("there was a problem while fetching coingecko client from context data."))
                };
                match stonk_client.stock_quote(&stonk_symbol.to_lowercase(), &country.to_lowercase()).await{
                    Ok(s) => {
                        stonk = s.clone();
                        match con.json_set(format!("stock:{stonk_symbol}:{country}"), "$",&s){
                            Ok(()) => match con.expire(format!("stock:{stonk_symbol}:{country}"), 300) {
                                Ok(()) =>  debug!("stored new cache for {stonk_symbol}:{country}."),
                                Err(y) => error!("{}",y)
                            },
                            Err(y) => error!("{}",y)
                        }
                    }
                    Err(y) => return Err(CommandError::from(format!("{}",y.to_string().to_lowercase())))
                }
            }
        }        
        
    }
    drop(context_data);
    msg.channel_id.send_message(ctx, |c|
        c.add_embed(|e| 
            e.description(format!(
            "** ​​  ​​ {} ​​  ​​  ​​ ({})**\n\n** ​​ {} ​​ {} {}**\n```24h: {}%```",
            stonk.name.clone(), stonk.symbol.clone(),
            check_direction(&stonk.percent_change.clone().parse().unwrap(),false),format_price(stonk.close.clone().parse().unwrap()).separate_with_commas(),stonk.currency.to_uppercase(),
            add_sign(stonk.percent_change.clone().parse().unwrap()),
            )).footer(|f|f.text(format!(" ​​  ​​ exchange: ​​  {}",stonk.exchange)))
        )
    ).await?;
    
    Ok(())
}

fn format_price(price: f64)-> String{
    if price < 0.00000000009{
        format!("{:.12}",price)
    }else if price < 0.0000009{
        format!("{:.9}",price)
    }else if price < 0.0009{
        format!("{:.6}",price)
    }else if price < 0.9{
        format!("{:.4}",price)
    }else if price < 9.99{
        format!("{:.3}",price)
    }else{
        format!("{:.2}",price)
    }
}

fn check_direction(number: &f64,ascii: bool) -> String{
    if number.is_sign_positive(){
        if ascii{
            return "⤤".to_string()
        }
        "<:upg:1121167053768900700>".to_string()
    }else{
        if ascii{
            return "⤥".to_string()
        }
        "<:red2:1121167795967770645>".to_string()
    }
}

fn add_sign(number: f64) -> String{
    if number.is_sign_positive(){
        format!("+{:.2}",number)
    }else{
        format!("{:.2}",number)
    }
}
