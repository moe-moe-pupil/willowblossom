use std::io::Read;

use curl::easy::{
    Easy,
    List,
};

#[test]
pub fn arrogance_ship() {
    let mut data = r#"{
  "model": "deepseek-chat",
  "prompt": "纯科幻太空世界，仅描述场景，不要续写任何人物故事，必须记住仅描述场景。狂妄号是一艘退役的太空战列舰，装载有大量火炮，不过在星际战争结束后就被封存了，如今狂妄号仅保留了极其坚固的外壳和能量护盾。狂妄号船身较长，内部通道众多。有热熔炸弹的自动生成工厂，太空服自动售货机，以及全舰的监控和可以上锁的自动太空门。飞船上没有任何npc，飞船收到了女皇号的求救信号，解除了全舱的休眠。你刚从休眠舱中醒来，你动了动还有些麻木的手脚，从休眠舱里起身，看见四周的休眠舱有几个早已打开，通向外侧的舱门也敞开着，你走了出去，通道上",
  "echo": false,
  "frequency_penalty": 0,
  "logprobs": 0,
  "max_tokens": 100,
  "presence_penalty": 0,
  "stop": null,
  "stream": false,
  "stream_options": null,
  "suffix": null,
  "temperature": 1.3,
  "top_p": 1
}"#
    .as_bytes();

    let mut easy = Easy::new();
    easy.url("https://api.deepseek.com/beta/completions")
        .unwrap();

    let mut list = List::new();
    list.append("Content-Type: application/json").unwrap();
    list.append("Accept: application/json").unwrap();
    list.append("Authorization: Bearer sk-04a95ae20ae24481a1908ba93be69de5")
        .unwrap();
    easy.http_headers(list).unwrap();
    easy.post(true).unwrap();
    easy.post_field_size(data.len() as u64).unwrap();

    // Perform the request and capture the response

    let mut dst = Vec::new();

    {
        let mut transfer = easy.transfer();
        transfer
            .read_function(|buf| Ok(data.read(buf).unwrap_or(0)))
            .unwrap();
        transfer
            .write_function(|data| {
                dst.extend_from_slice(data);
                Ok(data.len())
            })
            .unwrap();
        transfer.perform().unwrap();
    }

    println!(
        "HTML body {:#?}",
        String::from_utf8(dst).unwrap()
    );
}
