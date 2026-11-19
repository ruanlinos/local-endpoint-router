# Local Endpoint Router

<p>
  <img alt="Local Endpoint Router" src="src/assets/logo.png" width="640">
</p>

App desktop para desviar chamadas de microserviços remotos para processos locais, sem subir o ambiente inteiro.

Um pedido para `https://orders.company.com/api/orders` pode ir para `http://localhost:5001/api/orders`. O que não tem regra continua no ambiente compartilhado.

<p>
  <img alt="Tauri 2" src="https://img.shields.io/badge/Tauri-2-24C8DB?style=flat-square&logo=tauri&logoColor=white">
  <img alt="Rust" src="https://img.shields.io/badge/Rust-1.85+-000?style=flat-square&logo=rust&logoColor=white">
  <img alt="React" src="https://img.shields.io/badge/React-19-149ECA?style=flat-square&logo=react&logoColor=white">
  <img alt="TypeScript" src="https://img.shields.io/badge/TypeScript-5-3178C6?style=flat-square&logo=typescript&logoColor=white">
</p>

## Exemplo

| | |
| --- | --- |
| Origem | `https://payments.api.company.com` |
| Destino | `http://localhost:5002` |
| Pedido | `GET /api/payments/123?x=1` |
| Resultado | `GET http://localhost:5002/api/payments/123?x=1` |

Método, query e body seguem iguais. O header `Host` passa a ser o do destino. O host original vai em `X-Forwarded-Host`, junto com `X-Forwarded-For` e `X-Forwarded-Proto`.

## Por que existe

Em desenvolvimento, a maior parte dos serviços pode ficar no ambiente compartilhado. Só o serviço que você está mudando precisa rodar na máquina. O roteador fica no meio e troca o destino quando o host casa com uma regra.

## Como o tráfego entra

O listener padrão é `127.0.0.1:8787`. Ele não pede administrador.

1. Abra o app e inicie o proxy.
2. Crie um mapeamento com o host original e a URL local.
3. Ligue **Usar no navegador**. O proxy HTTP e HTTPS do sistema aponta para o app e volta ao valor anterior quando o proxy para.
4. Para HTTPS mapeado, confie na CA local. O app mostra o certificado como **Trusted** ou **Pending** antes de ligar a interceptação.

Chrome, Edge e Safari usam o proxy do sistema. No Firefox, a opção precisa estar em "usar proxy do sistema".

`localhost`, `127.0.0.1` e `::1` ficam no bypass. O processo Rust fala com o destino local direto, sem passar de novo pelo proxy.

## HTTP e HTTPS

| Pedido | O que acontece |
| --- | --- |
| HTTP com regra | Vai para o destino local |
| HTTP sem regra | Segue para o host original |
| `CONNECT` sem regra | Túnel de bytes. O certificado remoto permanece o original |
| `CONNECT` com regra e interceptação ligada | O TLS termina na CA local e o mesmo matcher escolhe o destino |
| `CONNECT` com regra e interceptação desligada | A conexão não segue para o remoto. O tráfego registra o erro |

A CA é uma por instalação, guardada no diretório de dados do app. A confiança é um gesto seu: login keychain no macOS, `CurrentUser\Root` no Windows. Host sem regra não recebe certificado forjado. A validação TLS dos upstreams que o próprio proxy abre continua ligada.

Um `301`, `302`, `303`, `307` ou `308` chega ao cliente com o `Location` original. O pedido seguinte passa de novo pelo proxy e, se o host estiver mapeado, cai no destino local.

## Telas

| Tela | Função |
| --- | --- |
| Dashboard | Status do proxy, proxy do sistema, estado da CA, mapeamentos ativos, tráfego recente |
| Mappings | Criar, editar, apagar, ligar e desligar regras sem reiniciar o listener |
| Traffic | Tabela ao vivo, inspector de headers e body, busca e filtros |
| Services | Saúde e latência dos destinos locais |
| Profiles | Conjuntos independentes de regras, com importação e exportação JSON |
| Settings | Endereço, porta, timeouts, gravação de tráfego e interceptação HTTPS |

Cada regra tem nome, host de origem (exato ou `*.sufixo`), porta, métodos, prefixo de path, reescrita opcional, destino, prioridade e timestamps. A prioridade menor ganha. No empate, o prefixo de path mais longo ganha, e o host exato ganha do wildcard.

O monitor guarda um anel de 500 entradas. Por padrão, `Authorization`, `Proxy-Authorization`, `Cookie` e `Set-Cookie` saem redatados, e o body completo não é gravado. O preview fica em 64 KiB.

## Desenvolvimento

Requisitos: Node.js, Rust 1.85+ e o toolchain nativo do sistema (Xcode Command Line Tools no macOS, Visual Studio Build Tools com MSVC no Windows).

```bash
npm install
npm run tauri dev
```

Verificação:

```bash
npm run check
cargo test --manifest-path src-tauri/Cargo.toml
```

## Binários

```bash
npm run tauri build
```

| Sistema | Artefato |
| --- | --- |
| macOS | `src-tauri/target/release/bundle/macos/Local Endpoint Router.app` |
| macOS | `src-tauri/target/release/bundle/dmg/` |
| Windows | `src-tauri/target/release/bundle/nsis/` |
| Windows | `src-tauri/target/release/local-endpoint-router.exe` |

Um build local não é assinado. No macOS, a primeira abertura pode exigir clique direito e **Abrir**.

## Segurança

- O bind padrão é loopback. Escutar fora dele é uma configuração explícita.
- Destino de regra aceita `http` e `https`. Loopback é o caso normal.
- Passthrough recusa loopback, rede privada, link-local e endereços de metadata. Um destino escrito na regra continua permitido.
- Metadata (`169.254.169.254`, `fd00:ec2::254`) e esquemas que não são HTTP são recusados.
- A CA não é instalada em silêncio e não entra na store da máquina.

HTTP/2 e WebSocket ficam fora deste corte. Site com certificate pinning recusa o certificado da CA local.

## Arquitetura

O Rust é dono do proxy, das regras, do SQLite, da saúde dos destinos, do proxy do sistema e do TLS. O React só chama commands e escuta eventos.

```text
src/                         interface
src-tauri/src/proxy/         listener HTTP/1.1, upstream, CONNECT
src-tauri/src/routing/       regras, matcher, ArcSwap
src-tauri/src/tls/           CA local e leaf por SNI
src-tauri/src/traffic/       anel, preview, redaction
src-tauri/src/system/        proxy do SO no macOS e no Windows
src-tauri/src/storage/       SQLite
```

O conjunto de regras ativo fica num `ArcSwap`. Ligar, desligar ou trocar de profile publica um snapshot novo. O listener não reinicia.

## Autor

**Ruan Linos Alves**

[LinkedIn](https://www.linkedin.com/in/linosruan) · [GitHub](https://github.com/ruanlinos)
