# Design: cliente SOAP rápido (substituto do SoapUI)

Data: 2026-10-29 · Status: **aprovado pelo usuário em 2026-10-29** · Nome do app: **Soapstone** **[usuário]**

Legenda: **[usuário]** = decidido pelo usuário · **[minha decisão]** = escolha minha, pode ser revista · sem marca = consequência direta do que foi decidido.

---

## 1. Intenção e porquê

O SoapUI (Java, SmartBear) é lento para tudo: abrir o app no Mac, navegar entre requisições, configurar ambientes. Ele é pesado porque carrega um runtime Java inteiro e compila todos os XSDs com XmlBeans.

O objetivo é um **"Insomnia para SOAP"**: colar a URL do WSDL e ver na hora todas as operações com o XML de exemplo já montado, editar, enviar e ver a resposta, com ambientes e variáveis no estilo Insomnia. Tem que ser **rápido e simples**, e rodar em Mac, Windows e Linux. **[usuário]**

O critério de sucesso é o usuário **nunca mais precisar abrir o SoapUI** para o que faz no dia a dia. **[usuário]**

O app será **gratuito e open source, público no GitHub**, para a comunidade usar. **[usuário]**

## 2. Etapas

| Etapa | Escopo |
|---|---|
| **1 (este documento)** | Requisições SOAP a partir de WSDL, ambientes/variáveis, autenticação (menos assinatura/criptografia WS-Security), importar projetos SoapUI, instaladores |
| **1.x** | WS-Security com assinatura e criptografia (keystores) |
| **2** | Test suites (asserções, sequência, transferência de valores) e mock services. Fica fora deste documento. **[usuário]** |

## 3. Objetivos da Etapa 1

1. **Carregar WSDL** por URL ou arquivo local, seguindo recursivamente `wsdl:import`, `xsd:import`, `xsd:include` (inclusive includes sem targetNamespace, os "chameleon"), com caminhos relativos.
2. **Listar serviços/bindings/operações** e gerar o **XML de exemplo** de cada operação no mesmo formato do SoapUI (detalhes na seção 7).
3. **Editar e enviar** a requisição; ver resposta formatada, status, tempo, tamanho, headers e o texto cru. SOAP Fault destacado.
4. **Ambientes e variáveis** no estilo Insomnia: `{{nome}}` em URL, headers, body e campos de autenticação. **[usuário]**
5. **Autenticação**: Basic, Bearer, headers HTTP customizados, certificado cliente (`.p12`/`.pfx`), NTLM, WS-Security UsernameToken (texto e digest) + Timestamp. **[usuário: prever os tipos para a comunidade; ordem de entrega é minha decisão]**
6. **Importar projetos `.xml` do SoapUI** para não precisar refazer nada. **[minha decisão, não contestada pelo usuário]**
7. **Instaladores** para os 3 sistemas, publicados no GitHub Releases.

## 4. Não-objetivos (Etapa 1)

- REST/JSON, GraphQL, JMS. **[usuário]**
- Testes de carga. **[usuário]**
- Test suites e mock services (Etapa 2). **[usuário]**
- WS-Security com assinatura/criptografia, SAML (Etapa 1.x).
- Scripts Groovy em qualquer forma.
- WS-Addressing, WS-ReliableMessaging, anexos MTOM/SwA. **[minha decisão]**: raros no público-alvo, entram sob demanda.
- Kerberos/SPNEGO, OAuth. **[minha decisão]**: idem.
- Keystore JKS: o usuário converte para `.p12` com `keytool`; o README explica. **[minha decisão]**
- Auto-update do app. **[minha decisão]**: o usuário baixa a nova versão no GitHub.
- Sincronização na nuvem e contas de usuário.
- Assinatura Apple e Windows no lançamento (app sai sem assinatura; README explica como liberar). **[usuário concordou]**

## 5. Anti-objetivos (falhou mesmo se "funcionar")

- Ser lento: abrir em vários segundos ou travar ao trocar de requisição.
- Ficar complexo: telas cheias de opções que o usuário comum nunca usa (o oposto do SoapUI).
- Gerar XML de exemplo diferente do SoapUI ou errado para WSDLs antigos (RPC/encoded). Quem migra do SoapUI tem que reconhecer o resultado.
- Exigir Java ou qualquer runtime instalado.

### Metas de desempenho **[minha decisão]**

Medidas num Mac Apple Silicon recente, build release:

| Ação | Meta |
|---|---|
| Abrir o app até a tela utilizável | < 1 s |
| Trocar de requisição na barra lateral | < 50 ms |
| Processar um WSDL grande já baixado (~5 MB, ~500 operações) | < 2 s |
| Gerar o XML de exemplo de uma operação | < 20 ms |

## 6. Stack e restrições

- **Núcleo em Rust, app com Tauri v2.** **[usuário]**
- **Interface:** React + TypeScript + Vite, editor **CodeMirror 6** com modo XML. **[minha decisão]**: React por ser o mais conhecido de quem vai contribuir; CodeMirror por ser leve.
- **Dados ficam dentro do app**, na máquina do usuário (pasta de dados do app), como no Insomnia. Nada de pasta de projeto nem git. **[usuário]**
  - Banco: SQLite (`rusqlite`). **[minha decisão]**
  - Valores de variáveis marcadas como **secretas** ficam no cofre do sistema (`keyring`: Keychain / Credential Manager / Secret Service). **[minha decisão]**: se o cofre não estiver disponível (ex.: Linux sem Secret Service), avisa e guarda no SQLite.
- **Licença MIT.** **[minha decisão]**
- **Nada de código copiado do SoapUI.** O SoapUI é EUPL 1.1 (copyleft); usamos só como referência de comportamento e como "gabarito" nos testes.
- **Interface só em inglês**, padrão da indústria. Sem sistema de tradução. **[usuário]**
- Código, commits e README em inglês. **[minha decisão]**
- **Toda comunicação de rede acontece no Rust** (baixar WSDL, enviar requisição), nunca no WebView. Isso evita CORS e é obrigatório para NTLM e certificado cliente.
- O **identificador do app** (ex.: `io.github.gsimei.soapstone`, conta do GitHub a confirmar) define a pasta onde os dados ficam. Ele precisa estar fechado **antes do primeiro release público**: trocar depois faz o usuário "perder" os dados.

### Bibliotecas Rust previstas

| Peça | Biblioteca | Observação |
|---|---|---|
| HTTP | `reqwest` | |
| XML (leitura/escrita) | `quick-xml`, `roxmltree` | |
| Modelo de tipos XSD | **resolvedor próprio sobre `roxmltree`** | Decidido no planejamento da Fase 1a. O `MetaTypes` do `xsd-parser` é voltado a gerar código Rust: converte tipos embutidos em tipos Rust (perde `xsd:int` etc., necessários no `xsi:type`) e descarta atributos estrangeiros como `wsdl:arrayType` (arrays SOAP). Precisaríamos de um adaptador e ainda ler o schema cru, então escrever o resolvedor sai menor |
| NTLM | `sspi` (Devolutions) | |
| Certificado cliente | `reqwest` (`native-tls` ou `rustls`, escolha do construtor) | PKCS#12 |
| WS-Security UsernameToken/Timestamp | código próprio | Pouco código: nonce, created, digest SHA-1 |
| Assinatura/criptografia XML (Etapa 1.x) | `bergshamra` | Nova (2026), passa a suíte de interoperabilidade do xmlsec |
| Cofre de senhas | `keyring` | |

## 7. Como o SoapUI faz (referência da engenharia reversa)

Arquivos de referência no repositório `smartbear/soapui`, pasta `soapui/src/main/java/com/eviware/soapui/impl/wsdl/support/`.

### 7.1 Carregar o WSDL (`xsd/SchemaUtils.java`, `wsdl/UrlWsdlLoader.java`)

- Baixa o documento raiz. Se for WSDL: extrai cada `<xs:schema>` de `<wsdl:types>` como um schema separado e segue `wsdl:import/@location`.
- Para cada schema, segue `xs:import/@schemaLocation` e `xs:include/@schemaLocation`. Caminho relativo é resolvido contra a URL do documento atual.
- **Include sem `targetNamespace`** recebe o namespace de quem incluiu.
- Pula imports de namespaces "embutidos": SOAP envelope/encoding 1.1 e 1.2, `xml.xsd`, `XMLSchema.xsd`, `xmime`, `xop`, `swaref`. Ele tem cópias locais desses schemas.
- Compila com regras frouxas: aceita schema que viola UPA/PVR e definições duplicadas no mesmo namespace. **Real-world WSDL é sujo; o nosso também tem que ser tolerante.**
- Detalhe que costuma quebrar: o schema extraído de dentro do WSDL depende de `xmlns:` declarados no `<wsdl:definitions>`. Esses prefixos precisam ser copiados para o schema extraído.
- Guarda **todas as partes baixadas** no projeto (`definitionCache`), por isso reabrir não precisa de rede. **Fazemos igual:** salvar no SQLite e ter um botão "Atualizar WSDL".

### 7.2 Gerar o XML de exemplo (`xsd/SampleXmlUtil.java`, `soap/SoapMessageBuilder.java`)

Envelope:
- `soapenv:Envelope` com o namespace da versão SOAP do binding (1.1 ou 1.2), `soapenv:Header` (vazio, ou com os headers declarados no binding) e `soapenv:Body`.
- **document/literal:** um elemento por `part` da mensagem de entrada (normalmente um só, o "wrapper").
- **RPC:** elemento raiz com o nome da operação, no namespace de `soap:body/@namespace` (se faltar, o targetNamespace do WSDL); um filho por `part`, **sem namespace**.
- **RPC/encoded** (`use="encoded"`): declara `xmlns:xsi` e `xmlns:xsd` no Envelope, adiciona `soapenv:encodingStyle` no elemento da operação e `xsi:type="prefixo:tipo"` em cada elemento com tipo nomeado. Elementos filhos sem namespace. `soapenc:Array` recebe `soapenc:arrayType="tipo[]"`.

Conteúdo, percorrendo o modelo de tipos:
- Valor de tipo simples: `?`. Para base64/hexBinary e tipos xmime: `cid:<número>`.
- Elemento com `default`: usa o valor default.
- Antes de elemento opcional: `<!--Optional:-->`. Repetível: `<!--Zero or more repetitions:-->`, `<!--N or more repetitions:-->` ou `<!--N to M repetitions:-->`.
- `choice`: `<!--You have a CHOICE of the next N items at this level-->` e gera **todas** as opções.
- `all`: `<!--You may enter the following N items in any order-->`.
- `any`: `<!--You may enter ANY elements at this point-->` e não gera nada.
- Atributos: gera todos, com valor default ou `?`.
- Ocorrência: se min == max, gera min; senão gera `max(min, 1)` (opcionais são incluídos).
- **Recursão:** se um tipo já está na pilha de tipos sendo gerados, para (evita loop infinito).
- Conteúdo misto (`mixed`): o SoapUI insere palavras aleatórias; nós deixamos vazio. **[minha decisão]**
- Saída indentada ("pretty print").

Exemplo real do usuário (RPC/encoded, operação `AVITIK_GETGROUPRESERVES` do WSDL Optitravel), no formato que o gerador precisa produzir. O nome do prefixo (`opti`, `wbs`, `tns`...) pode variar; o resto não:

```xml
<soapenv:Envelope xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:xsd="http://www.w3.org/2001/XMLSchema" xmlns:soapenv="http://schemas.xmlsoap.org/soap/envelope/" xmlns:opti="http://www2.optitravel.net/optitravel/export/wbs/opti_res/">
   <soapenv:Header/>
   <soapenv:Body>
      <opti:AVITIK_GETGROUPRESERVES soapenv:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
         <Credential xsi:type="opti:Credentials">
            <SystemID xsi:type="xsd:int">?</SystemID>
            <Username xsi:type="xsd:string">?</Username>
            <Password xsi:type="xsd:string">?</Password>
         </Credential>
         ...
      </opti:AVITIK_GETGROUPRESERVES>
   </soapenv:Body>
</soapenv:Envelope>
```

### 7.3 Envio HTTP

- SOAP 1.1: `Content-Type: text/xml;charset=UTF-8` e header `SOAPAction: "<soapAction do binding>"`.
- SOAP 1.2: `Content-Type: application/soap+xml;charset=UTF-8;action="<soapAction>"`.
- Endpoint padrão: `soap:address/@location` do `wsdl:port`. O usuário pode trocar (normalmente por `{{baseUrl}}...`).

### 7.4 Autenticação no SoapUI

- HTTP: Basic, Digest, NTLM (via JCIFS), SPNEGO/Kerberos, OAuth.
- WS-Security (Apache WSS4J): UsernameToken (PasswordText, PasswordDigest, com Nonce e Created opcionais), Timestamp (TTL), Signature, Encryption, SAML. Keystores JKS/PKCS#12.

### 7.5 Formato do projeto SoapUI (`soapui/src/main/xsd/soapui/soapui.xsd`)

Raiz `con:soapui-project` (namespace `http://eviware.com/soapui/config`):
- `con:properties` → propriedades do projeto (nome/valor).
- `con:interface` (tipo WsdlInterface): `definition` (URL), `bindingName`, `soapVersion`; contém `con:definitionCache` (todas as partes do WSDL/XSD já baixadas, com URL e conteúdo), `con:endpoints` e `con:operation`.
- `con:operation` → `con:call` (requisição salva): `name`, `con:endpoint`, `con:request` (XML do body), `con:credentials` (username, password, domain, authType), `outgoingWss`, `wssPasswordType`.
- Também: `con:testSuite`, `con:mockService`, `con:wssContainer` (configurações WS-Security do projeto).

## 8. Como o app funciona

### 8.1 Modelo de dados

```
Projeto
 ├─ Variáveis base (nome, valor, secreta?)
 ├─ Ambientes (ex.: dev, homolog, prod) → variáveis que sobrescrevem a base
 ├─ Ambiente ativo
 └─ Serviços (um por WSDL importado)
      ├─ URL de origem + cache de todas as partes baixadas
      └─ Operações (binding, nome, versão SOAP, soapAction, endpoint padrão)
           └─ Requisições salvas (nome, endpoint, headers, body, auth, WS-Security, última resposta)
```

- O app tem **vários projetos**, listados na barra lateral. **[minha decisão]**
- Uma operação recém-importada ganha uma requisição "Request 1" com o XML de exemplo, como no SoapUI.
- Guarda **só a última resposta** de cada requisição. **[minha decisão]**: histórico completo fica para depois.
- **Atualizar WSDL**: baixa de novo, atualiza a lista de operações e **não mexe** nas requisições já salvas. Operação que sumiu fica marcada como órfã, não é apagada.

### 8.2 Variáveis

- Sintaxe `{{nome}}`. Funciona em: endpoint, headers, body e todos os campos de autenticação/WS-Security.
- Resolução: ambiente ativo → variáveis base do projeto.
- Variável não definida: destacada no editor; ao enviar, mostra erro listando as que faltam e **não envia**. **[minha decisão]**
- Variável secreta: valor no cofre do sistema, mostrado mascarado na tela.

### 8.3 Sequência de envio

```
body da requisição
  → substituir variáveis
  → aplicar WS-Security (se configurado): insere <wsse:Security> no Header
  → montar a requisição HTTP (Content-Type, SOAPAction, headers do usuário)
  → aplicar autenticação HTTP (Basic / Bearer / NTLM / certificado)
  → enviar → mostrar resposta
```

Cada tipo de autenticação é uma peça independente nessa sequência. A assinatura WS-Security (Etapa 1.x) entra como mais uma peça depois do UsernameToken/Timestamp, sem mudar o resto.

### 8.4 Autenticação na Etapa 1

| Tipo | Onde vai | Detalhes |
|---|---|---|
| Basic | HTTP | Enviado já na primeira requisição (preemptivo) |
| Bearer | HTTP | `Authorization: Bearer {{token}}` |
| Headers customizados | HTTP | Lista nome/valor |
| Certificado cliente | TLS | Arquivo `.p12`/`.pfx` + senha |
| NTLM | HTTP | Usuário, senha, domínio. **Atenção:** o NTLM exige que as 3 trocas do handshake usem a mesma conexão TCP |
| WS-Security UsernameToken | Header SOAP | PasswordText ou PasswordDigest (`Base64(SHA-1(nonce + created + senha))`), Nonce e Created opcionais |
| WS-Security Timestamp | Header SOAP | Created/Expires com TTL configurável |

Opção "ignorar erros de certificado TLS" por requisição (útil em homologação). **[minha decisão]**

Rede **[minha decisão]**:
- Proxy: respeita o proxy do sistema e as variáveis `HTTP_PROXY`/`HTTPS_PROXY`/`NO_PROXY`. Tela de proxy manual fica para depois.
- Timeout padrão de 60 s, configurável por requisição. Botão para cancelar envio em andamento.

### 8.5 Importar projeto SoapUI

- Lê o `.xml` e cria um Projeto: serviços a partir de `definitionCache` (funciona offline, sem baixar nada), operações, requisições salvas (body, endpoint, credenciais HTTP, UsernameToken).
- Propriedades do projeto viram **variáveis base**.
- `${#Project#nome}` vira `{{nome}}` em todos os campos.
- Outras expansões (`${#TestCase#...}`, `${#Global#...}`, `${=groovy}`) ficam como estão e aparecem num relatório de importação.
- Test suites, mock services e configurações WS-Security de assinatura são ignorados, com aviso: "X test suites ignoradas, chegam na Etapa 2".

### 8.6 Interface (estilo Insomnia)

**[minha decisão, sujeita a mockup]**
- **Barra lateral:** Projeto → Serviços → Operações → Requisições, com busca.
- **Topo:** seletor de ambiente e botão para editar ambientes.
- **Centro:** endpoint + botão Enviar; abas Body (XML), Headers, Auth, WS-Security.
- **Direita:** resposta (XML formatado, Headers, Cru), com status, tempo e tamanho.
- **Atalhos:** `Cmd/Ctrl+Enter` envia; `Cmd/Ctrl+P` abre uma busca rápida de operações/requisições. Isso ataca a dor de "navegar entre requisições".
- Importar: botão "Novo serviço" aceita URL do WSDL, arquivo local ou projeto SoapUI.
- Tema claro/escuro seguindo o sistema.

## 9. Fases de construção da Etapa 1

| Fase | Entrega | Como validar |
|---|---|---|
| **1a. Prova do núcleo** | Biblioteca Rust: WSDL (URL/arquivo) → operações → XML de exemplo. Um executável de teste: `<url> → imprime operações e XMLs` | Testes "gabarito" (seção 10) passando para todo o corpus. Plano: `docs/superpowers/plans/2026-10-29-soapstone-phase-1a-core.md` |
| **1b. App mínimo** | Tauri: importar WSDL, barra lateral, editor, enviar, resposta, projetos, ambientes, variáveis, Basic/Bearer/headers, persistência SQLite | Fluxo completo contra serviços públicos; metas de desempenho medidas |
| **1c. Autenticação** | Certificado cliente, WS-Security UsernameToken/Timestamp, NTLM, variáveis secretas no cofre | Testes contra servidores locais (ex.: WS-Security com um serviço de teste; NTLM com IIS/Windows ou servidor de teste) |
| **1d. Migração e distribuição** | Importar projeto SoapUI; GitHub Actions gerando `.dmg` (Apple Silicon + Intel), `.msi`/`.exe`, `.AppImage`/`.deb`; README com instruções de instalação sem assinatura | Importar projetos reais do usuário; instalar nos 3 sistemas |

**Etapa 1.x:** WS-Security Signature e Encryption com `bergshamra` (tela de keystore `.p12`, algoritmos, partes a assinar).

## 10. Testes

- **Gabarito do SoapUI:** para cada WSDL do corpus, o XML de exemplo gerado pelo SoapUI 5.9.1 (instalado na máquina do usuário em `/Applications/SoapUI-5.9.1.app`) é salvo como arquivo esperado. O nosso gerador tem que produzir o mesmo XML, ignorando diferenças só de espaço em branco e de ordem/nome de prefixos de namespace. **[minha decisão]**: os gabaritos podem ser gerados rodando o próprio `.jar` do SoapUI instalado com um script pequeno, ou à mão pela interface. O SoapUI serve só como oráculo nos testes: não é distribuído nem copiado.
- **Corpus mínimo de WSDLs:**
  - document/literal wrapped simples (ex.: calculadora pública);
  - WSDL com XSDs em arquivos separados, com `import` e `include` relativos (estilo WCF/.NET);
  - **RPC/encoded do usuário (obrigatório):** `https://www2.optitravel.net/optitravel/export/wbs/opti_res/wbs_server.php?wsdl` (público, ~77 KB). Tem 18 operações, `style="rpc"`, `use="encoded"`, um schema inline com 95 tipos e **23 arrays SOAP** no formato `<restriction base="soapenc:Array"><attribute ref="soapenc:arrayType" wsdl:arrayType="tns:Traveller[]"/>`. Usa o prefixo `soapenc` **sem `xs:import`**, então o carregador precisa ter o schema de SOAP encoding embutido e tolerar o import ausente, como o SoapUI faz. Salvar uma cópia como fixture para os testes não dependerem da rede;
  - RPC/literal;
  - SOAP 1.2;
  - tipo recursivo (elemento que contém a si mesmo);
  - `choice`, `all`, `any`, extensão/restrição de tipo complexo, `attributeGroup`, `group`, `ref`;
  - o WSDL real do usuário, se ele puder fornecer.
- Envelope e autenticação: testes de unidade comparando XML/headers gerados (UsernameToken digest com nonce/created fixos).

## 11. Riscos

1. **WSDLs reais são sujos.** Schemas inválidos, prefixos herdados do WSDL, includes chameleon. Mitigação: compilação tolerante, Fase 1a dedicada a isso, corpus de testes.
2. **Resolvedor XSD próprio** precisa cobrir as construções reais (extensão, grupos, refs, includes chameleon, arrays SOAP). Mitigação: corpus de testes com gabarito do SoapUI; construções fora do corpus geram `?` em vez de falhar.
3. **NTLM e reaproveitamento de conexão** no `reqwest`. Mitigação: cliente dedicado por requisição NTLM, HTTP/1.1, uma conexão.
4. **WebView no Linux (WebKitGTK)** é mais lento que no Mac/Windows. Mitigação: medir as metas também no Linux; manter a interface leve.
5. **App sem assinatura** assusta usuário leigo. Mitigação: instruções claras no README; assinar quando o projeto crescer.

## 12. Deixado para o construtor

- Organização interna do código (workspace Rust, módulos), desde que o núcleo seja uma biblioteca testável sem a interface.
- Esquema do SQLite.
- `native-tls` vs `rustls`.
- Gerenciamento de estado e estilo no React.
